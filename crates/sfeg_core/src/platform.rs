//! What the runner and renderer use from the core (ADR 0007).
//!
//! This is separate from the game-facing API. Games draw through
//! [`DrawContext`](crate::DrawContext); the runner collects the result with
//! [`Core::draw`](crate::Core::draw) and hands it to the renderer as a
//! [`DrawList`]: plain data, with nothing that depends on wgpu (ADR 0006).

use std::num::NonZeroU32;

use bytemuck::NoUninit;

use crate::camera::Projection;
use crate::color::Color;
use crate::input::KeyCode;
use crate::math::{Matrix4, Rotation, Vector3};

/// Something the player did, as the runner reports it to
/// [`Core::apply_input_event`](crate::Core::apply_input_event)
/// (ADR 0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputEvent {
    /// A key went down. Only real presses, not the operating system's key
    /// repeat while held.
    KeyPressed(KeyCode),
    /// A key came up.
    KeyReleased(KeyCode),
    /// The window lost focus, so it won't hear about keys released until it
    /// gets focus back.
    FocusLost,
}

/// Everything one frame draws: where the camera is, and what is in front
/// of it.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawList {
    camera: Option<CameraView>,
    draw_commands: Vec<DrawCommand>,
}

impl DrawList {
    /// A frame with no camera, which draws nothing (ADR 0024).
    pub(crate) fn without_camera() -> DrawList {
        DrawList {
            camera: None,
            draw_commands: Vec::new(),
        }
    }

    pub(crate) fn new(camera: CameraView, draw_commands: Vec<DrawCommand>) -> DrawList {
        DrawList {
            camera: Some(camera),
            draw_commands,
        }
    }

    /// The camera the frame is seen from, or `None` if the scene has no
    /// active camera. Without one, there are no draw commands either.
    pub fn camera(&self) -> Option<&CameraView> {
        self.camera.as_ref()
    }

    /// What the objects drew, in the order they drew it.
    pub fn draw_commands(&self) -> &[DrawCommand] {
        &self.draw_commands
    }
}

/// Where the active camera is this frame, and how it projects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    view_matrix: Matrix4,
    projection: Projection,
}

impl CameraView {
    /// A camera at `world_position`, turned by `world_rotation`, looking
    /// down its own −Z axis (ADR 0024).
    pub(crate) fn new(
        world_position: Vector3,
        world_rotation: Rotation,
        projection: Projection,
    ) -> CameraView {
        // The inverse of moving to the camera's place: undo the move, then
        // undo the turn. Scale is ignored, so this is exact.
        let view = glam::Mat4::from_quat(world_rotation.inverse().to_glam())
            * glam::Mat4::from_translation(-world_position.to_glam());
        CameraView {
            view_matrix: Matrix4::from_glam(view),
            projection,
        }
    }

    /// The matrix from world space to the camera's space, where the camera
    /// is at the origin looking down −Z.
    pub fn view_matrix(&self) -> Matrix4 {
        self.view_matrix
    }

    /// How the view maps onto the screen.
    pub fn projection(&self) -> Projection {
        self.projection
    }

    /// The matrix from world space to clip space, for a render target
    /// `width_px` by `height_px` pixels. The aspect ratio comes from the
    /// target, so the view is never stretched.
    pub fn view_projection(&self, width_px: NonZeroU32, height_px: NonZeroU32) -> ViewProjection {
        // Lossy for huge sizes, but both are rounded the same way and the
        // ratio stays positive and finite.
        let aspect_ratio = width_px.get() as f32 / height_px.get() as f32;
        ViewProjection(self.projection.to_glam(aspect_ratio) * self.view_matrix.to_glam())
    }
}

/// A matrix from world space to clip space: a camera's view followed by its
/// projection.
///
/// Unlike [`Matrix4`], it isn't affine, so it has its own type. It can be
/// cast to bytes for the GPU, in column-major order, but not created from
/// bytes.
#[derive(Debug, Clone, Copy, PartialEq, NoUninit)]
#[repr(transparent)]
pub struct ViewProjection(glam::Mat4);

impl ViewProjection {
    /// The elements in column-major order, as the GPU expects.
    pub fn to_cols_array(self) -> [f32; 16] {
        self.0.to_cols_array()
    }

    /// Where a world position lands in normalized device coordinates: x and
    /// y from −1 to 1 across the screen, +Y up, and depth from 0 at the near
    /// plane to 1 at the far plane.
    pub fn project_point(self, world_position: Vector3) -> Vector3 {
        Vector3::from_glam(self.0.project_point3(world_position.to_glam()))
    }
}

/// One thing to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum DrawCommand {
    /// A built-in mesh in one flat color.
    FlatMesh {
        /// Which mesh.
        mesh: BuiltinMesh,
        /// Where the mesh is in the world, already interpolated.
        world_matrix: Matrix4,
        /// Its color.
        color: Color,
    },
}

/// A mesh the renderer always has, with no asset to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BuiltinMesh {
    /// A cube one unit on each side, centered on the origin.
    Cube,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Orthographic, Perspective};
    use crate::math::assert_close;

    fn size(width: u32, height: u32) -> (NonZeroU32, NonZeroU32) {
        (
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        )
    }

    fn assert_vector_close(a: Vector3, b: Vector3) {
        assert_close(a.x(), b.x());
        assert_close(a.y(), b.y());
        assert_close(a.z(), b.z());
    }

    #[test]
    fn the_camera_looks_down_minus_z() {
        let perspective = Perspective::new(1.0, 1.0, 100.0).unwrap();
        let camera = CameraView::new(
            Vector3::new(0.0, 0.0, 10.0),
            Rotation::IDENTITY,
            perspective.into(),
        );
        let (width, height) = size(800, 600);
        let clip = camera.view_projection(width, height);
        // The world origin is ten units in front of the camera, at the
        // center of the screen.
        let center = clip.project_point(Vector3::ZERO);
        assert_close(center.x(), 0.0);
        assert_close(center.y(), 0.0);
        assert!(center.z() > 0.0 && center.z() < 1.0, "{center:?}");
        // The near plane is at depth 0 and the far plane at depth 1.
        assert_close(clip.project_point(Vector3::new(0.0, 0.0, 9.0)).z(), 0.0);
        assert_close(clip.project_point(Vector3::new(0.0, 0.0, -90.0)).z(), 1.0);
        // +Y is up on screen.
        assert!(clip.project_point(Vector3::UNIT_Y).y() > 0.0);
    }

    #[test]
    fn the_view_follows_the_camera_rotation() {
        // Turned a quarter-turn left, the camera looks down world −X.
        let turned = Rotation::from_rotation_y(std::f32::consts::FRAC_PI_2).unwrap();
        let orthographic = Orthographic::new(2.0, 0.0, 100.0).unwrap();
        let camera = CameraView::new(Vector3::ZERO, turned, orthographic.into());
        let in_view = camera
            .view_matrix()
            .transform_point(Vector3::new(-5.0, 0.0, 0.0));
        assert_vector_close(in_view, Vector3::new(0.0, 0.0, -5.0));
    }

    #[test]
    fn the_aspect_ratio_comes_from_the_target() {
        let orthographic = Orthographic::new(2.0, -1.0, 1.0).unwrap();
        let camera = CameraView::new(Vector3::ZERO, Rotation::IDENTITY, orthographic.into());
        // Two units tall on a target twice as wide as tall shows four
        // units across, so x = 2 is the right edge.
        let (width, height) = size(200, 100);
        let edge = camera
            .view_projection(width, height)
            .project_point(Vector3::new(2.0, 1.0, 0.0));
        assert_close(edge.x(), 1.0);
        assert_close(edge.y(), 1.0);
    }
}
