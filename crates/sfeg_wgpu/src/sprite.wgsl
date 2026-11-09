// Textured squares: sprites.
//
// Group 0 holds the camera and group 1 the per-object data, as ADR 0018
// reserves them for the engine. Group 2 holds the sprite's texture, where
// a material's values will go.

struct Camera {
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

struct Sprite {
    world: mat4x4<f32>,
}

@group(1) @binding(0)
var<storage, read> sprites: array<Sprite>;

@group(2) @binding(0)
var sprite_texture: texture_2d<f32>;

@group(2) @binding(1)
var sprite_sampler: sampler;

// A square one unit on each side, centered on the origin in the XY plane,
// as two triangles counter-clockwise seen from +Z.
const CORNERS = array<vec2<f32>, 6>(
    vec2<f32>(-0.5, -0.5),
    vec2<f32>(0.5, -0.5),
    vec2<f32>(0.5, 0.5),
    vec2<f32>(-0.5, -0.5),
    vec2<f32>(0.5, 0.5),
    vec2<f32>(-0.5, 0.5),
);

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex: u32,
    @builtin(instance_index) instance: u32,
) -> VertexOutput {
    let corner = CORNERS[vertex];
    var out: VertexOutput;
    out.clip_position = camera.view_projection * sprites[instance].world * vec4<f32>(corner, 0.0, 1.0);
    // The texture's top row is at +Y, and its left column at -X.
    out.uv = vec2<f32>(corner.x + 0.5, 0.5 - corner.y);
    return out;
}

// Pixels less than this opaque are cut out, so sprites need no sorting
// against each other. A placeholder until materials choose blending
// (ADR 0018).
const ALPHA_CUTOFF = 0.5;

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(sprite_texture, sprite_sampler, in.uv);
    if color.a < ALPHA_CUTOFF {
        discard;
    }
    return vec4<f32>(color.rgb, 1.0);
}
