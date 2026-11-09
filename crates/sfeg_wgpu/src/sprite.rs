//! Draws sprites: textures on flat squares, one draw call per sprite.
//!
//! The renderer keeps its own copy of each texture on the GPU, keyed by
//! the core's handle (ADR 0006). A sprite whose texture the renderer
//! doesn't have, such as one still loading, draws a placeholder.

use std::collections::HashMap;

use sfeg_core::assets::{DecodedTexture, TextureHandle};
use sfeg_core::platform::{DrawCommand, DrawList, TextureChange};
use wgpu::util::DeviceExt;

use crate::flat_mesh::DEPTH_FORMAT;

/// One sprite's data on the GPU: its world matrix. Matches `Sprite` in
/// `sprite.wgsl`.
type SpriteData = [f32; 16];

/// How many sprites the buffer holds before it first has to grow.
const INITIAL_CAPACITY: usize = 64;

/// Vertices in a sprite's square: two triangles.
const SQUARE_VERTICES: u32 = 6;

/// The placeholder is a checkerboard this many pixels on each side.
const PLACEHOLDER_SIDE_PX: u32 = 8;

/// The placeholder's two colors, as sRGB RGBA8: magenta and black, which
/// few games use, so a missing texture stands out.
const PLACEHOLDER_COLORS: [[u8; 4]; 2] = [[255, 0, 255, 255], [0, 0, 0, 255]];

/// Which texture one sprite draws with.
#[derive(Clone, Copy)]
enum SpriteTexture {
    Uploaded(TextureHandle),
    Placeholder,
}

pub(crate) struct SpritePipeline {
    pipeline: wgpu::RenderPipeline,
    sprites_layout: wgpu::BindGroupLayout,
    sprites_buffer: wgpu::Buffer,
    sprites_capacity: usize,
    sprites_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    placeholder: wgpu::BindGroup,
    /// Each uploaded texture's bind group, which keeps the texture alive.
    uploaded: HashMap<TextureHandle, wgpu::BindGroup>,
    /// The sprites uploaded for this frame, in draw order.
    sprite_data: Vec<SpriteData>,
    /// The texture each of them draws with.
    sprite_textures: Vec<SpriteTexture>,
}

impl SpritePipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color_format: wgpu::TextureFormat,
        camera_layout: &wgpu::BindGroupLayout,
    ) -> SpritePipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sfeg sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });
        let sprites_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sfeg sprites layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sfeg sprite texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // Nearest, for crisp pixel art, until materials choose (ADR 0018).
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sfeg sprite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sfeg sprite layout"),
            bind_group_layouts: &[
                Some(camera_layout),
                Some(&sprites_layout),
                Some(&texture_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sfeg sprite"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Both sides show, so a sprite turned away or mirrored
                // stays visible.
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let (sprites_buffer, sprites_bind_group) =
            create_sprites_buffer(device, &sprites_layout, INITIAL_CAPACITY);
        let placeholder_pixels = placeholder_rgba8();
        let placeholder = create_texture_bind_group(
            device,
            queue,
            &texture_layout,
            &sampler,
            "sfeg placeholder texture",
            PLACEHOLDER_SIDE_PX,
            PLACEHOLDER_SIDE_PX,
            &placeholder_pixels,
        );

        SpritePipeline {
            pipeline,
            sprites_layout,
            sprites_buffer,
            sprites_capacity: INITIAL_CAPACITY,
            sprites_bind_group,
            texture_layout,
            sampler,
            placeholder,
            uploaded: HashMap::new(),
            sprite_data: Vec::new(),
            sprite_textures: Vec::new(),
        }
    }

    /// Uploads textures that became ready and frees ones that were freed.
    pub(crate) fn apply_texture_changes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        changes: Vec<TextureChange>,
    ) {
        for change in changes {
            match change {
                TextureChange::Ready { texture, decoded } => {
                    let bind_group = self.upload(device, queue, &decoded);
                    self.uploaded.insert(texture, bind_group);
                }
                TextureChange::Freed { texture } => {
                    // Dropping the bind group drops the last reference to
                    // the GPU texture.
                    self.uploaded.remove(&texture);
                }
                // The core and renderer ship together, so a change this
                // renderer doesn't know can't reach it.
                _ => {}
            }
        }
    }

    /// Uploads the sprites `draw_list` holds. A draw list without a camera
    /// draws nothing.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draw_list: &DrawList,
    ) {
        self.sprite_data.clear();
        self.sprite_textures.clear();
        if draw_list.camera().is_none() {
            return;
        }
        for command in draw_list.draw_commands() {
            if let DrawCommand::Sprite {
                texture,
                world_matrix,
            } = *command
            {
                self.sprite_data.push(world_matrix.to_cols_array());
                self.sprite_textures
                    .push(if self.uploaded.contains_key(&texture) {
                        SpriteTexture::Uploaded(texture)
                    } else {
                        SpriteTexture::Placeholder
                    });
            }
        }
        if self.sprite_data.is_empty() {
            return;
        }
        if self.sprite_data.len() > self.sprites_capacity {
            let capacity = self.sprite_data.len().next_power_of_two();
            (self.sprites_buffer, self.sprites_bind_group) =
                create_sprites_buffer(device, &self.sprites_layout, capacity);
            self.sprites_capacity = capacity;
        }
        queue.write_buffer(
            &self.sprites_buffer,
            0,
            bytemuck::cast_slice(&self.sprite_data),
        );
    }

    /// Records the draws for what [`SpritePipeline::prepare`] uploaded.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        camera_bind_group: &wgpu::BindGroup,
    ) {
        if self.sprite_data.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, camera_bind_group, &[]);
        pass.set_bind_group(1, &self.sprites_bind_group, &[]);
        for (index, texture) in self.sprite_textures.iter().enumerate() {
            let bind_group = match texture {
                SpriteTexture::Uploaded(texture) => &self.uploaded[texture],
                SpriteTexture::Placeholder => &self.placeholder,
            };
            let instance =
                u32::try_from(index).expect("the GPU can't hold more sprites than fit in a u32");
            pass.set_bind_group(2, bind_group, &[]);
            pass.draw(0..SQUARE_VERTICES, instance..instance + 1);
        }
    }

    fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        decoded: &DecodedTexture,
    ) -> wgpu::BindGroup {
        create_texture_bind_group(
            device,
            queue,
            &self.texture_layout,
            &self.sampler,
            "sfeg sprite texture",
            decoded.width_px().get(),
            decoded.height_px().get(),
            decoded.rgba8(),
        )
    }
}

/// An 8×8 checkerboard of the two placeholder colors.
fn placeholder_rgba8() -> Vec<u8> {
    (0..PLACEHOLDER_SIDE_PX * PLACEHOLDER_SIDE_PX)
        .flat_map(|index| {
            let (x, y) = (index % PLACEHOLDER_SIDE_PX, index / PLACEHOLDER_SIDE_PX);
            PLACEHOLDER_COLORS[((x + y) % 2) as usize]
        })
        .collect()
}

/// Uploads `rgba8`, sRGB-encoded and `width_px` by `height_px`, and binds
/// it with `sampler` for the sprite shader.
#[expect(
    clippy::too_many_arguments,
    reason = "each is a separate piece of GPU state the upload needs"
)]
fn create_texture_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    label: &str,
    width_px: u32,
    height_px: u32,
    rgba8: &[u8],
) -> wgpu::BindGroup {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: width_px,
                height: height_px,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba8,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_sprites_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    capacity: usize,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sfeg sprites"),
        size: (capacity * std::mem::size_of::<SpriteData>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sfeg sprites"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, bind_group)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_is_valid_wgsl() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("sprite.wgsl"))
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string("sprite.wgsl")));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the shader validates");
    }

    #[test]
    fn the_placeholder_is_a_checkerboard() {
        let pixels = placeholder_rgba8();
        assert_eq!(
            pixels.len(),
            (PLACEHOLDER_SIDE_PX * PLACEHOLDER_SIDE_PX * 4) as usize
        );
        assert_eq!(pixels[0..4], PLACEHOLDER_COLORS[0]);
        assert_eq!(pixels[4..8], PLACEHOLDER_COLORS[1]);
        let second_row = (PLACEHOLDER_SIDE_PX * 4) as usize;
        assert_eq!(pixels[second_row..second_row + 4], PLACEHOLDER_COLORS[1]);
    }
}
