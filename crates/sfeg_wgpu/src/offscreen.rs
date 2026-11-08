//! Rendering with no window: into a texture that is read back to the CPU.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::mpsc;

use sfeg_core::platform::DrawList;

use crate::flat_mesh::FlatMeshPipeline;
use crate::{create_depth_view, encode_world_pass, log_adapter, request_device};

/// The offscreen target's format: 8-bit RGBA, sRGB-encoded. Reading it
/// back gives bytes ready for a PNG, with no conversion.
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Bytes in one RGBA8 pixel.
const BYTES_PER_PIXEL: u32 = 4;

/// Draws frames into an image instead of a window, then reads each one
/// back to the CPU.
///
/// It draws through the same pipelines as the windowed [`crate::Renderer`],
/// so a captured image shows what the window would. The only difference is
/// the format: the window may use BGRA, while captures are always RGBA.
///
/// It needs no window or display server, only a GPU driver. A CPU driver,
/// such as Mesa's lavapipe, works, and is picked when nothing else can
/// draw. The picked adapter is logged.
pub struct OffscreenRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width_px: NonZeroU32,
    height_px: NonZeroU32,
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    /// Where each frame is copied to be read back. Its rows are padded to
    /// [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`].
    readback_buffer: wgpu::Buffer,
    padded_bytes_per_row: u32,
    flat_meshes: FlatMeshPipeline,
}

impl OffscreenRenderer {
    /// Creates a renderer whose images are `width_px` by `height_px`
    /// pixels.
    ///
    /// This blocks until the GPU is ready.
    ///
    /// # Errors
    ///
    /// - [`OffscreenRendererError::NoAdapter`] if no GPU can draw.
    /// - [`OffscreenRendererError::RequestDevice`] if the GPU refused to
    ///   open.
    pub fn new(
        width_px: NonZeroU32,
        height_px: NonZeroU32,
    ) -> Result<OffscreenRenderer, OffscreenRendererError> {
        // Environment variables such as WGPU_BACKEND can override the
        // defaults, which helps when debugging a driver.
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        // With no surface to match, wgpu ranks CPU adapters last but still
        // picks one when it is the only adapter.
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|source| OffscreenRendererError::NoAdapter(Box::new(source)))?;
        log_adapter(&adapter);
        let (device, queue) = request_device(&adapter)
            .map_err(|source| OffscreenRendererError::RequestDevice(Box::new(source)))?;

        let size = wgpu::Extent3d {
            width: width_px.get(),
            height: height_px.get(),
            depth_or_array_layers: 1,
        };
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sfeg offscreen color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = create_depth_view(&device, width_px.get(), height_px.get());

        let padded_bytes_per_row =
            (width_px.get() * BYTES_PER_PIXEL).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sfeg offscreen readback"),
            size: u64::from(padded_bytes_per_row) * u64::from(height_px.get()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let flat_meshes = FlatMeshPipeline::new(&device, OFFSCREEN_FORMAT);
        Ok(OffscreenRenderer {
            device,
            queue,
            width_px,
            height_px,
            color_texture,
            color_view,
            depth_view,
            readback_buffer,
            padded_bytes_per_row,
            flat_meshes,
        })
    }

    /// Draws `draw_list` as one frame, then blocks until the GPU has
    /// finished and returns the frame as an image.
    ///
    /// A draw list without a camera gives an image of only the clear color
    /// (ADR 0024).
    ///
    /// # Errors
    ///
    /// - [`RenderToImageError::WaitForGpu`] if the GPU failed while
    ///   finishing the frame, such as after a driver crash.
    /// - [`RenderToImageError::ReadBack`] if the frame couldn't be copied
    ///   back to the CPU.
    pub fn render_to_image(
        &mut self,
        draw_list: &DrawList,
    ) -> Result<CapturedImage, RenderToImageError> {
        self.flat_meshes.prepare(
            &self.device,
            &self.queue,
            draw_list,
            self.width_px,
            self.height_px,
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sfeg offscreen frame"),
            });
        encode_world_pass(
            &mut encoder,
            &self.flat_meshes,
            &self.color_view,
            &self.depth_view,
        );
        encoder.copy_texture_to_buffer(
            self.color_texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height_px.get()),
                },
            },
            self.color_texture.size(),
        );
        self.queue.submit([encoder.finish()]);

        let (sender, receiver) = mpsc::channel();
        self.readback_buffer
            .map_async(wgpu::MapMode::Read, .., move |result| {
                // The receiver is waiting below, so this only fails if it
                // was dropped, which it isn't.
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|source| RenderToImageError::WaitForGpu(Box::new(source)))?;
        // Waiting ran the callback, so the result is already there.
        receiver
            .try_recv()
            .map_err(|_| RenderToImageError::ReadBack(Box::new(CallbackNotRun)))?
            .map_err(|source| RenderToImageError::ReadBack(Box::new(source)))?;

        let row_bytes = (self.width_px.get() * BYTES_PER_PIXEL) as usize;
        let mut rgba8 = Vec::with_capacity(row_bytes * self.height_px.get() as usize);
        let copied = match self.readback_buffer.get_mapped_range(..) {
            Ok(padded) => {
                for row in padded.chunks_exact(self.padded_bytes_per_row as usize) {
                    rgba8.extend_from_slice(&row[..row_bytes]);
                }
                Ok(())
            }
            Err(source) => Err(RenderToImageError::ReadBack(Box::new(source))),
        };
        // Unmapped either way, so the next frame can map it again.
        self.readback_buffer.unmap();
        copied?;
        Ok(CapturedImage {
            width_px: self.width_px,
            height_px: self.height_px,
            rgba8,
        })
    }
}

/// The GPU said it had finished, but the read-back callback hadn't run.
/// wgpu promises it has, so this means a wgpu bug.
#[derive(Debug)]
struct CallbackNotRun;

impl fmt::Display for CallbackNotRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the GPU finished, but the read-back callback didn't run")
    }
}

impl Error for CallbackNotRun {}

/// One rendered frame, read back from the GPU.
///
/// Pixels are 8-bit RGBA, sRGB-encoded, in rows from the top of the image
/// to the bottom, left to right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedImage {
    width_px: NonZeroU32,
    height_px: NonZeroU32,
    rgba8: Vec<u8>,
}

impl CapturedImage {
    /// The width, in pixels.
    pub fn width_px(&self) -> NonZeroU32 {
        self.width_px
    }

    /// The height, in pixels.
    pub fn height_px(&self) -> NonZeroU32 {
        self.height_px
    }

    /// Every pixel, 4 bytes each, from the top row down.
    pub fn rgba8(&self) -> &[u8] {
        &self.rgba8
    }

    /// The pixel `x_px` from the left and `y_px` from the top, or `None`
    /// if that is outside the image.
    pub fn pixel_rgba8(&self, x_px: u32, y_px: u32) -> Option<[u8; 4]> {
        if x_px >= self.width_px.get() || y_px >= self.height_px.get() {
            return None;
        }
        let start = ((y_px * self.width_px.get() + x_px) * BYTES_PER_PIXEL) as usize;
        let mut pixel = [0; 4];
        pixel.copy_from_slice(&self.rgba8[start..start + BYTES_PER_PIXEL as usize]);
        Some(pixel)
    }

    /// Writes the image to `path` as a PNG, replacing any file already
    /// there.
    ///
    /// # Errors
    ///
    /// - [`CapturedImageError::CreateFile`] if the file couldn't be
    ///   created, such as when its folder doesn't exist.
    /// - [`CapturedImageError::EncodePng`] if writing the PNG failed, such
    ///   as when the disk is full.
    pub fn write_png(&self, path: &Path) -> Result<(), CapturedImageError> {
        let file = File::create(path).map_err(CapturedImageError::CreateFile)?;
        let mut encoder = png::Encoder::new(
            BufWriter::new(file),
            self.width_px.get(),
            self.height_px.get(),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = encoder
            .write_header()
            .map_err(|source| CapturedImageError::EncodePng(Box::new(source)))?;
        writer
            .write_image_data(&self.rgba8)
            .map_err(|source| CapturedImageError::EncodePng(Box::new(source)))?;
        writer
            .finish()
            .map_err(|source| CapturedImageError::EncodePng(Box::new(source)))
    }
}

/// Why an [`OffscreenRenderer`] could not be created.
#[derive(Debug)]
#[non_exhaustive]
pub enum OffscreenRendererError {
    /// No GPU on this machine can draw, such as on a machine without any
    /// graphics driver, not even a CPU one.
    NoAdapter(Box<dyn Error + Send + Sync>),
    /// A GPU was found but refused to open, such as after a driver crash.
    RequestDevice(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for OffscreenRendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OffscreenRendererError::NoAdapter(_) => write!(f, "no GPU can draw offscreen"),
            OffscreenRendererError::RequestDevice(_) => write!(f, "the GPU refused to open"),
        }
    }
}

impl Error for OffscreenRendererError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            OffscreenRendererError::NoAdapter(source)
            | OffscreenRendererError::RequestDevice(source) => Some(source.as_ref()),
        }
    }
}

/// Why [`OffscreenRenderer::render_to_image`] failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum RenderToImageError {
    /// The GPU failed while finishing the frame, such as after a driver
    /// crash.
    WaitForGpu(Box<dyn Error + Send + Sync>),
    /// The finished frame couldn't be copied back to the CPU.
    ReadBack(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for RenderToImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderToImageError::WaitForGpu(_) => {
                write!(f, "the GPU failed while finishing the frame")
            }
            RenderToImageError::ReadBack(_) => {
                write!(f, "couldn't read the frame back from the GPU")
            }
        }
    }
}

impl Error for RenderToImageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RenderToImageError::WaitForGpu(source) | RenderToImageError::ReadBack(source) => {
                Some(source.as_ref())
            }
        }
    }
}

/// Why [`CapturedImage::write_png`] failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum CapturedImageError {
    /// The file couldn't be created, such as when its folder doesn't exist
    /// or isn't writable.
    CreateFile(std::io::Error),
    /// Encoding or writing the PNG failed, such as when the disk is full.
    EncodePng(Box<dyn Error + Send + Sync>),
}

impl fmt::Display for CapturedImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapturedImageError::CreateFile(_) => write!(f, "couldn't create the image file"),
            CapturedImageError::EncodePng(_) => write!(f, "couldn't write the PNG"),
        }
    }
}

impl Error for CapturedImageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            CapturedImageError::CreateFile(source) => Some(source),
            CapturedImageError::EncodePng(source) => Some(source.as_ref()),
        }
    }
}
