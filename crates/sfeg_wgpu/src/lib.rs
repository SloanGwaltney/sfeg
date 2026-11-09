//! SFEG's renderer.
//!
//! It renders to a surface created from any raw window handle with wgpu
//! (ADR 0007). It must never depend on winit, so the runner hands it a
//! window through the `raw-window-handle` traits.
//!
//! It can also render offscreen, with no window, into an image that is
//! read back to the CPU ([`OffscreenRenderer`]). Headless runs and tests use
//! it to see what a frame shows.
//!
//! Each frame clears the target to [`PLACEHOLDER_CLEAR_COLOR`], then draws
//! the core's [`DrawList`] from its camera.

mod flat_mesh;
mod offscreen;
mod sprite;
mod world;

use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;

use sfeg_core::platform::{DrawList, TextureChange};
use wgpu::rwh::{HasDisplayHandle, HasWindowHandle};

use crate::flat_mesh::DEPTH_FORMAT;
use crate::world::WorldPipelines;

pub use crate::offscreen::{
    CapturedImage, CapturedImageError, OffscreenRenderer, OffscreenRendererError,
    RenderToImageError,
};

/// The color every frame is cleared to, until the game can choose its own.
/// A dark blue-gray, in linear color.
pub const PLACEHOLDER_CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.03,
    b: 0.05,
    a: 1.0,
};

/// A window the renderer can draw into.
///
/// Anything with both raw handles works, such as `Arc<winit::window::Window>`.
/// The renderer keeps it, so it can recreate the surface if the platform
/// loses it.
pub trait RenderWindow: HasWindowHandle + HasDisplayHandle + fmt::Debug + Send + Sync {}

impl<T: HasWindowHandle + HasDisplayHandle + fmt::Debug + Send + Sync> RenderWindow for T {}

/// Draws frames into one window.
pub struct Renderer {
    /// Shared, because the window has several owners that must all keep it
    /// alive: the runner uses it every frame, the surface draws into it, and
    /// the renderer keeps it to recreate a lost surface. Borrowing instead
    /// would make the surface borrow from a sibling field, which safe Rust
    /// can't express. `Arc`, not `Rc`, because wgpu requires window handles
    /// to be `Send + Sync`.
    window: Arc<dyn RenderWindow>,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    instance: wgpu::Instance,
    /// Matches the surface's size. `None` while the window has no area.
    depth_view: Option<wgpu::TextureView>,
    world: WorldPipelines,
}

impl Renderer {
    /// Creates a renderer for `window`, whose drawable area is
    /// `width_px` by `height_px` physical pixels.
    ///
    /// This blocks until the GPU is ready. On macOS it must be called on
    /// the main thread.
    ///
    /// # Errors
    ///
    /// - [`RendererError::CreateSurface`] if wgpu can't draw into the
    ///   window.
    /// - [`RendererError::NoAdapter`] if no GPU can draw into it.
    /// - [`RendererError::RequestDevice`] if the GPU refused to open.
    /// - [`RendererError::SurfaceUnsupported`] if the GPU can't present to
    ///   the window.
    pub fn new(
        window: Arc<dyn RenderWindow>,
        width_px: u32,
        height_px: u32,
    ) -> Result<Renderer, RendererError> {
        // Environment variables such as WGPU_BACKEND can override the
        // defaults, which helps when debugging a driver.
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let surface = instance
            .create_surface(window.clone())
            .map_err(|source| RendererError::CreateSurface(Box::new(source)))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|source| RendererError::NoAdapter(Box::new(source)))?;
        log_adapter(&adapter);
        let (device, queue) = request_device(&adapter)
            .map_err(|source| RendererError::RequestDevice(Box::new(source)))?;

        let capabilities = surface.get_capabilities(&adapter);
        // Prefer an sRGB format, so colors are blended in linear space and
        // shown correctly.
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(RendererError::SurfaceUnsupported)?;
        let alpha_mode = *capabilities
            .alpha_modes
            .first()
            .ok_or(RendererError::SurfaceUnsupported)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width_px,
            height: height_px,
            desired_maximum_frame_latency: 2,
            // Vsync, which every platform supports.
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode,
            view_formats: vec![],
        };

        let world = WorldPipelines::new(&device, &queue, format);
        let mut renderer = Renderer {
            window,
            surface,
            adapter,
            device,
            queue,
            config,
            instance,
            depth_view: None,
            world,
        };
        renderer.configure_surface();
        Ok(renderer)
    }

    /// Tells the renderer the window's drawable area is now `width_px` by
    /// `height_px` physical pixels. Zero in either, such as while
    /// minimized, skips drawing until the size is non-zero again.
    pub fn resize(&mut self, width_px: u32, height_px: u32) {
        self.config.width = width_px;
        self.config.height = height_px;
        self.configure_surface();
    }

    /// Uploads textures that became ready and frees ones that were freed,
    /// from [`Core::drain_texture_changes_for_renderer`](sfeg_core::Core::drain_texture_changes_for_renderer).
    /// Call it before each frame, so sprites draw with their textures as
    /// soon as they are ready.
    pub fn apply_texture_changes(&mut self, changes: Vec<TextureChange>) {
        self.world
            .apply_texture_changes(&self.device, &self.queue, changes);
    }

    /// Draws `draw_list` as one frame and presents it.
    ///
    /// A draw list without a camera only clears the window (ADR 0024).
    /// Frames the platform can't take right now, such as while the window
    /// is hidden, are skipped. Problems that fix themselves, such as an
    /// outdated surface after a resize, are fixed for the next frame.
    pub fn render_frame(&mut self, draw_list: &DrawList) {
        if !self.has_area() {
            return;
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Draw this one, then match the surface for the next.
                self.draw(frame, draw_list);
                self.configure_surface();
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure_surface();
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.recreate_surface();
                return;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                tracing::warn!(
                    "skipped a frame because getting the window's surface failed validation"
                );
                return;
            }
        };
        self.draw(frame, draw_list);
    }

    fn draw(&mut self, frame: wgpu::SurfaceTexture, draw_list: &DrawList) {
        let (Some(width_px), Some(height_px), Some(depth_view)) = (
            NonZeroU32::new(self.config.width),
            NonZeroU32::new(self.config.height),
            &self.depth_view,
        ) else {
            // Only reached with an area, which configuring gave a depth
            // buffer.
            return;
        };
        self.world
            .prepare(&self.device, &self.queue, draw_list, width_px, height_px);
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sfeg frame"),
            });
        self.world.encode_pass(&mut encoder, &view, depth_view);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }

    fn has_area(&self) -> bool {
        self.config.width > 0 && self.config.height > 0
    }

    fn configure_surface(&mut self) {
        if !self.has_area() {
            self.depth_view = None;
            return;
        }
        self.surface.configure(&self.device, &self.config);
        self.depth_view = Some(create_depth_view(
            &self.device,
            self.config.width,
            self.config.height,
        ));
    }

    fn recreate_surface(&mut self) {
        match self.instance.create_surface(self.window.clone()) {
            Ok(surface)
                if surface
                    .get_capabilities(&self.adapter)
                    .formats
                    .contains(&self.config.format) =>
            {
                self.surface = surface;
                self.configure_surface();
            }
            Ok(_) => tracing::warn!(
                "the window's surface was lost and the new one has a different format; frames are skipped"
            ),
            Err(error) => {
                tracing::warn!(%error, "the window's surface was lost and couldn't be recreated; frames are skipped")
            }
        }
    }
}

/// Logs which GPU the renderer picked, so a frame can be traced back to
/// the driver that drew it. CPU adapters, such as Mesa's lavapipe, are
/// only picked when nothing else can draw.
fn log_adapter(adapter: &wgpu::Adapter) {
    let info = adapter.get_info();
    tracing::info!(
        name = %info.name,
        device_type = ?info.device_type,
        backend = ?info.backend,
        driver = %info.driver,
        driver_info = %info.driver_info,
        "picked a GPU adapter"
    );
}

/// Opens `adapter`, blocking until it is ready.
fn request_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("sfeg device"),
        ..Default::default()
    }))
}

/// A depth buffer `width_px` by `height_px` pixels, for one render target.
fn create_depth_view(device: &wgpu::Device, width_px: u32, height_px: u32) -> wgpu::TextureView {
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sfeg depth"),
        size: wgpu::Extent3d {
            width: width_px,
            height: height_px,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    depth.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Why a [`Renderer`] could not be created.
#[derive(Debug)]
#[non_exhaustive]
pub enum RendererError {
    /// wgpu couldn't create a surface for the window, such as when the
    /// window handle isn't one it supports.
    CreateSurface(Box<dyn Error + Send + Sync>),
    /// No GPU on this machine can draw into the window, such as on a
    /// machine without working graphics drivers.
    NoAdapter(Box<dyn Error + Send + Sync>),
    /// A GPU was found but refused to open, such as after a driver crash.
    RequestDevice(Box<dyn Error + Send + Sync>),
    /// The GPU can draw but can't present to this window.
    SurfaceUnsupported,
}

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RendererError::CreateSurface(_) => {
                write!(f, "couldn't create a surface for the window")
            }
            RendererError::NoAdapter(_) => write!(f, "no GPU can draw into the window"),
            RendererError::RequestDevice(_) => write!(f, "the GPU refused to open"),
            RendererError::SurfaceUnsupported => {
                write!(f, "the GPU can't present to the window")
            }
        }
    }
}

impl Error for RendererError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RendererError::CreateSurface(source)
            | RendererError::NoAdapter(source)
            | RendererError::RequestDevice(source) => Some(source.as_ref()),
            RendererError::SurfaceUnsupported => None,
        }
    }
}
