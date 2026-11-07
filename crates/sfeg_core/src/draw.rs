use crate::color::Color;
use crate::math::Matrix4;
use crate::platform::{BuiltinMesh, DrawCommand};

/// What a `draw` callback can do: record what the object looks like this
/// frame (ADR 0008).
///
/// Drawing never calls the GPU. It records draw commands, which the runner
/// hands to the renderer (ADR 0006).
pub struct DrawContext<'a> {
    draw_commands: &'a mut Vec<DrawCommand>,
    world_matrix: Matrix4,
    interpolation_factor: f32,
}

impl<'a> DrawContext<'a> {
    pub(crate) fn new(
        draw_commands: &'a mut Vec<DrawCommand>,
        world_matrix: Matrix4,
        interpolation_factor: f32,
    ) -> DrawContext<'a> {
        DrawContext {
            draw_commands,
            world_matrix,
            interpolation_factor,
        }
    }

    /// Draws a cube one unit on each side in one flat `color`, centered on
    /// this object and placed by its world transform.
    ///
    /// The transform is interpolated between the last two ticks, unless
    /// interpolation is turned off (ADR 0013). The object's scale sizes the
    /// cube, so a scale of 2 draws a cube two units on each side.
    pub fn draw_cube(&mut self, color: Color) {
        self.draw_commands.push(DrawCommand::FlatMesh {
            mesh: BuiltinMesh::Cube,
            world_matrix: self.world_matrix,
            color,
        });
    }

    /// How far real time has moved past the last tick, as a fraction of one
    /// tick, from 0 up to but not including 1.
    ///
    /// Transforms are already interpolated by it. Use it to blend the
    /// object's own values between ticks.
    pub fn interpolation_factor(&self) -> f32 {
        self.interpolation_factor
    }
}
