// Built-in meshes in one flat color.
//
// Group 0 holds the camera and group 1 the per-object data, as ADR 0018
// reserves them for the engine.

struct Camera {
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

struct FlatMesh {
    world: mat4x4<f32>,
    // Linear RGB. Alpha is unused, since the mesh is opaque.
    color: vec4<f32>,
}

@group(1) @binding(0)
var<storage, read> flat_meshes: array<FlatMesh>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) @interpolate(flat) color: vec3<f32>,
}

@vertex
fn vertex_main(
    @location(0) local_position: vec3<f32>,
    @builtin(instance_index) instance: u32,
) -> VertexOutput {
    let mesh = flat_meshes[instance];
    let world_position = mesh.world * vec4<f32>(local_position, 1.0);
    var out: VertexOutput;
    out.clip_position = camera.view_projection * world_position;
    out.world_position = world_position.xyz;
    out.color = mesh.color.rgb;
    return out;
}

// A placeholder until lighting is designed: each face is shaded by how
// much it faces a fixed direction, so a cube's faces can be told apart.
const PLACEHOLDER_LIGHT_DIRECTION = vec3<f32>(0.4, 1.0, 0.7);

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // The face's normal, from how the world position changes across the
    // screen. It is right under any scale and needs no normal data.
    let normal = normalize(cross(dpdx(in.world_position), dpdy(in.world_position)));
    let facing = abs(dot(normal, normalize(PLACEHOLDER_LIGHT_DIRECTION)));
    return vec4<f32>(in.color * mix(0.55, 1.0, facing), 1.0);
}
