//! Everything that draws the world: the camera, and a pipeline for each
//! kind of draw command. The window and offscreen renderers both draw
//! through it, so a capture shows exactly what the window would.

use std::num::NonZeroU32;

use sfeg_core::platform::{DrawList, TextureChange};

use crate::PLACEHOLDER_CLEAR_COLOR;
use crate::flat_mesh::FlatMeshPipeline;
use crate::sprite::SpritePipeline;

pub(crate) struct WorldPipelines {
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    flat_meshes: FlatMeshPipeline,
    sprites: SpritePipeline,
}

impl WorldPipelines {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color_format: wgpu::TextureFormat,
    ) -> WorldPipelines {
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sfeg camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sfeg camera"),
            size: std::mem::size_of::<[f32; 16]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sfeg camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        WorldPipelines {
            camera_buffer,
            camera_bind_group,
            flat_meshes: FlatMeshPipeline::new(device, color_format, &camera_layout),
            sprites: SpritePipeline::new(device, queue, color_format, &camera_layout),
        }
    }

    /// Uploads textures that became ready and frees ones that were freed.
    pub(crate) fn apply_texture_changes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        changes: Vec<TextureChange>,
    ) {
        self.sprites.apply_texture_changes(device, queue, changes);
    }

    /// Uploads the camera and everything `draw_list` holds, for a target
    /// `width_px` by `height_px`.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draw_list: &DrawList,
        width_px: NonZeroU32,
        height_px: NonZeroU32,
    ) {
        if let Some(camera) = draw_list.camera() {
            queue.write_buffer(
                &self.camera_buffer,
                0,
                bytemuck::bytes_of(&camera.view_projection(width_px, height_px)),
            );
        }
        self.flat_meshes.prepare(device, queue, draw_list);
        self.sprites.prepare(device, queue, draw_list);
    }

    /// Records the pass that clears `color_view` and draws what
    /// [`WorldPipelines::prepare`] uploaded into it.
    pub(crate) fn encode_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sfeg world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(PLACEHOLDER_CLEAR_COLOR),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    // Nothing reads depth after this pass.
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        self.flat_meshes.draw(&mut pass, &self.camera_bind_group);
        self.sprites.draw(&mut pass, &self.camera_bind_group);
    }
}
