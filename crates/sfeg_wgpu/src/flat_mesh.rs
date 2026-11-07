//! Draws built-in meshes in one flat color, all in one instanced draw call
//! per mesh.

use std::num::NonZeroU32;

use sfeg_core::platform::{BuiltinMesh, DrawCommand, DrawList};
use wgpu::util::DeviceExt;

/// The depth buffer's format.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// One mesh's data on the GPU: its world matrix, then its linear color
/// with an unused alpha. Matches `FlatMesh` in `flat_mesh.wgsl`.
type FlatMeshData = [f32; 20];

/// How many meshes the buffer holds before it first has to grow.
const INITIAL_CAPACITY: usize = 64;

/// A cube one unit on each side, centered on the origin.
const CUBE_CORNERS: [[f32; 3]; 8] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [0.5, 0.5, -0.5],
    [-0.5, 0.5, -0.5],
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [0.5, 0.5, 0.5],
    [-0.5, 0.5, 0.5],
];

/// Two triangles per face, counter-clockwise seen from outside.
#[rustfmt::skip]
const CUBE_INDICES: [u16; 36] = [
    4, 5, 6, 4, 6, 7, // +Z
    1, 0, 3, 1, 3, 2, // -Z
    5, 1, 2, 5, 2, 6, // +X
    0, 4, 7, 0, 7, 3, // -X
    7, 6, 2, 7, 2, 3, // +Y
    0, 1, 5, 0, 5, 4, // -Y
];

pub(crate) struct FlatMeshPipeline {
    pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    meshes_layout: wgpu::BindGroupLayout,
    meshes_buffer: wgpu::Buffer,
    meshes_capacity: usize,
    meshes_bind_group: wgpu::BindGroup,
    cube_vertices: wgpu::Buffer,
    cube_indices: wgpu::Buffer,
    /// The cubes uploaded for this frame.
    cube_data: Vec<FlatMeshData>,
}

impl FlatMeshPipeline {
    pub(crate) fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
    ) -> FlatMeshPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sfeg flat mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("flat_mesh.wgsl").into()),
        });

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
        let meshes_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sfeg flat meshes layout"),
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
        let (meshes_buffer, meshes_bind_group) =
            create_meshes_buffer(device, &meshes_layout, INITIAL_CAPACITY);

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sfeg flat mesh layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&meshes_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sfeg flat mesh"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<[f32; 3]>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // Not culled, so a mirrored object, whose triangles wind
                // the other way, still shows its outside. The depth test
                // hides the inside.
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

        let cube_vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sfeg cube vertices"),
            contents: bytemuck::cast_slice(&CUBE_CORNERS),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let cube_indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sfeg cube indices"),
            contents: bytemuck::cast_slice(&CUBE_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        FlatMeshPipeline {
            pipeline,
            camera_buffer,
            camera_bind_group,
            meshes_layout,
            meshes_buffer,
            meshes_capacity: INITIAL_CAPACITY,
            meshes_bind_group,
            cube_vertices,
            cube_indices,
            cube_data: Vec::new(),
        }
    }

    /// Uploads the camera and meshes `draw_list` holds, for a target
    /// `width_px` by `height_px`.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draw_list: &DrawList,
        width_px: NonZeroU32,
        height_px: NonZeroU32,
    ) {
        self.cube_data.clear();
        let Some(camera) = draw_list.camera() else {
            return;
        };
        queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&camera.view_projection(width_px, height_px)),
        );
        self.cube_data.extend(flat_cubes(draw_list.draw_commands()));
        if self.cube_data.is_empty() {
            return;
        }
        if self.cube_data.len() > self.meshes_capacity {
            let capacity = self.cube_data.len().next_power_of_two();
            (self.meshes_buffer, self.meshes_bind_group) =
                create_meshes_buffer(device, &self.meshes_layout, capacity);
            self.meshes_capacity = capacity;
        }
        queue.write_buffer(
            &self.meshes_buffer,
            0,
            bytemuck::cast_slice(&self.cube_data),
        );
    }

    /// Records the draw for what [`FlatMeshPipeline::prepare`] uploaded.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.cube_data.is_empty() {
            return;
        }
        let instances = u32::try_from(self.cube_data.len())
            .expect("the GPU can't hold more meshes than fit in a u32");
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.meshes_bind_group, &[]);
        pass.set_vertex_buffer(0, self.cube_vertices.slice(..));
        pass.set_index_buffer(self.cube_indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..CUBE_INDICES.len() as u32, 0, 0..instances);
    }
}

/// The GPU data for every flat cube in `draw_commands`, in draw order.
fn flat_cubes(draw_commands: &[DrawCommand]) -> impl Iterator<Item = FlatMeshData> + '_ {
    draw_commands.iter().filter_map(|command| match *command {
        DrawCommand::FlatMesh {
            mesh: BuiltinMesh::Cube,
            world_matrix,
            color,
        } => {
            let mut data = [0.0; 20];
            data[..16].copy_from_slice(&world_matrix.to_cols_array());
            data[16..19].copy_from_slice(&color.to_linear_array());
            data[19] = 1.0;
            Some(data)
        }
        // The core and renderer ship together, so a command this renderer
        // doesn't know can't reach it.
        _ => None,
    })
}

fn create_meshes_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    capacity: usize,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sfeg flat meshes"),
        size: (capacity * std::mem::size_of::<FlatMeshData>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sfeg flat meshes"),
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
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("flat_mesh.wgsl"))
            .unwrap_or_else(|error| panic!("{}", error.emit_to_string("flat_mesh.wgsl")));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the shader validates");
    }

    #[test]
    fn the_cube_winds_outward() {
        for face in CUBE_INDICES.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| CUBE_CORNERS[usize::from(face[i])]);
            let normal = cross(sub(b, a), sub(c, a));
            let center = [0, 1, 2].map(|axis| (a[axis] + b[axis] + c[axis]) / 3.0);
            // The normal points away from the cube's center.
            let outward: f32 = (0..3).map(|axis| normal[axis] * center[axis]).sum();
            assert!(outward > 0.0, "{face:?}");
        }
    }

    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
}
