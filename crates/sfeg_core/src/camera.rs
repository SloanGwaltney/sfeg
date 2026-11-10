//! Projections and the active camera (ADR 0024).
//!
//! Any game object can be the camera. The scene holds one active camera:
//! the object to look from and the [`Projection`] to look through. The view
//! follows the object's interpolated world position and rotation, and
//! ignores its scale.
//!
//! The world is right-handed with +Y up, and a camera looks down its own
//! −Z axis.

use std::error::Error;
use std::f32::consts::PI;
use std::fmt;

use serde::Deserialize;

use crate::handle::ObjectHandle;

/// A perspective projection: things farther away look smaller, as in most
/// 3D games.
///
/// The width shown follows from the height and the render target's aspect
/// ratio, which is read each frame, so resizing never distorts the view.
///
/// In scene files it is written with named fields, such as
/// `Perspective(vertical_fov_radians: 1.0472, near: 0.1, far: 100.0)`, and
/// checked like [`Perspective::new`].
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "PerspectiveFields")]
pub struct Perspective {
    vertical_fov_radians: f32,
    near: f32,
    far: f32,
}

impl Perspective {
    /// A perspective that shows `vertical_fov_radians` from the bottom of
    /// the screen to the top, and things from `near` to `far` world units in
    /// front of the camera.
    ///
    /// # Errors
    ///
    /// - [`ProjectionError::NotFinite`] if any value is NaN or infinite.
    /// - [`ProjectionError::FieldOfViewOutOfRange`] if the field of view
    ///   isn't more than zero and less than a half-turn (π radians).
    /// - [`ProjectionError::NearNotPositive`] if `near` is zero or negative.
    /// - [`ProjectionError::FarNotPastNear`] if `far` isn't more than `near`.
    pub fn new(
        vertical_fov_radians: f32,
        near: f32,
        far: f32,
    ) -> Result<Perspective, ProjectionError> {
        if ![vertical_fov_radians, near, far]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(ProjectionError::NotFinite);
        }
        if vertical_fov_radians <= 0.0 || vertical_fov_radians >= PI {
            return Err(ProjectionError::FieldOfViewOutOfRange);
        }
        if near <= 0.0 {
            return Err(ProjectionError::NearNotPositive);
        }
        if far <= near {
            return Err(ProjectionError::FarNotPastNear);
        }
        let perspective = Perspective {
            vertical_fov_radians,
            near,
            far,
        };
        // Extreme but in-range values, such as a field of view of 1e-39,
        // can still overflow the matrix.
        if !perspective.to_glam(1.0).is_finite() {
            return Err(ProjectionError::NotFinite);
        }
        Ok(perspective)
    }

    /// The angle from the bottom of the screen to the top, in radians.
    pub fn vertical_fov_radians(&self) -> f32 {
        self.vertical_fov_radians
    }

    /// How far in front of the camera things start to show, in world units.
    pub fn near(&self) -> f32 {
        self.near
    }

    /// How far in front of the camera things stop showing, in world units.
    pub fn far(&self) -> f32 {
        self.far
    }

    fn to_glam(self, aspect_ratio: f32) -> glam::Mat4 {
        // wgpu, like Direct3D, has +Y up and depth from 0 to 1.
        glam::camera::rh::proj::directx::perspective(
            self.vertical_fov_radians,
            aspect_ratio,
            self.near,
            self.far,
        )
    }
}

/// An orthographic projection: things look the same size at any distance,
/// as in 2D games, isometric views, and technical drawings.
///
/// The width shown follows from the height and the render target's aspect
/// ratio, which is read each frame, so resizing never distorts the view.
///
/// In scene files it is written with named fields, such as
/// `Orthographic(visible_height: 10.0, near: 0.0, far: 100.0)`, and
/// checked like [`Orthographic::new`].
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "OrthographicFields")]
pub struct Orthographic {
    visible_height: f32,
    near: f32,
    far: f32,
}

impl Orthographic {
    /// An orthographic projection that shows `visible_height` world units
    /// from the bottom of the screen to the top, and things from `near` to
    /// `far` world units in front of the camera.
    ///
    /// Unlike [`Perspective`], `near` may be zero or negative, which shows
    /// things slightly behind the camera too.
    ///
    /// # Errors
    ///
    /// - [`ProjectionError::NotFinite`] if any value is NaN or infinite.
    /// - [`ProjectionError::VisibleHeightNotPositive`] if `visible_height`
    ///   is zero or negative.
    /// - [`ProjectionError::FarNotPastNear`] if `far` isn't more than `near`.
    pub fn new(visible_height: f32, near: f32, far: f32) -> Result<Orthographic, ProjectionError> {
        if ![visible_height, near, far]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(ProjectionError::NotFinite);
        }
        if visible_height <= 0.0 {
            return Err(ProjectionError::VisibleHeightNotPositive);
        }
        if far <= near {
            return Err(ProjectionError::FarNotPastNear);
        }
        let orthographic = Orthographic {
            visible_height,
            near,
            far,
        };
        if !orthographic.to_glam(1.0).is_finite() {
            return Err(ProjectionError::NotFinite);
        }
        Ok(orthographic)
    }

    /// How many world units are shown from the bottom of the screen to the
    /// top.
    pub fn visible_height(&self) -> f32 {
        self.visible_height
    }

    /// How far in front of the camera things start to show, in world units.
    pub fn near(&self) -> f32 {
        self.near
    }

    /// How far in front of the camera things stop showing, in world units.
    pub fn far(&self) -> f32 {
        self.far
    }

    fn to_glam(self, aspect_ratio: f32) -> glam::Mat4 {
        let half_height = self.visible_height / 2.0;
        let half_width = half_height * aspect_ratio;
        glam::camera::rh::proj::directx::orthographic(
            -half_width,
            half_width,
            -half_height,
            half_height,
            self.near,
            self.far,
        )
    }
}

/// How a camera's view maps onto the screen: one of the projection types.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum Projection {
    /// See [`Perspective`].
    Perspective(Perspective),
    /// See [`Orthographic`].
    Orthographic(Orthographic),
}

impl Projection {
    pub(crate) fn to_glam(self, aspect_ratio: f32) -> glam::Mat4 {
        match self {
            Projection::Perspective(perspective) => perspective.to_glam(aspect_ratio),
            Projection::Orthographic(orthographic) => orthographic.to_glam(aspect_ratio),
        }
    }
}

impl From<Perspective> for Projection {
    fn from(perspective: Perspective) -> Projection {
        Projection::Perspective(perspective)
    }
}

impl From<Orthographic> for Projection {
    fn from(orthographic: Orthographic) -> Projection {
        Projection::Orthographic(orthographic)
    }
}

/// Why a projection could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectionError {
    /// A value is NaN or infinite, or so extreme the projection overflows.
    NotFinite,
    /// A perspective field of view is zero or less, or a half-turn or more.
    FieldOfViewOutOfRange,
    /// A perspective near plane is zero or behind the camera.
    NearNotPositive,
    /// The far plane is at or in front of the near plane.
    FarNotPastNear,
    /// An orthographic visible height is zero or negative.
    VisibleHeightNotPositive,
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectionError::NotFinite => {
                write!(f, "a projection value is not finite, or overflows")
            }
            ProjectionError::FieldOfViewOutOfRange => write!(
                f,
                "the field of view must be more than zero and less than a half-turn"
            ),
            ProjectionError::NearNotPositive => {
                write!(f, "a perspective near plane must be more than zero")
            }
            ProjectionError::FarNotPastNear => {
                write!(f, "the far plane must be farther than the near plane")
            }
            ProjectionError::VisibleHeightNotPositive => {
                write!(f, "an orthographic visible height must be more than zero")
            }
        }
    }
}

impl Error for ProjectionError {}

/// The serialized form of [`Perspective`], with named fields.
#[derive(Deserialize)]
#[serde(rename = "Perspective", deny_unknown_fields)]
struct PerspectiveFields {
    vertical_fov_radians: f32,
    near: f32,
    far: f32,
}

impl TryFrom<PerspectiveFields> for Perspective {
    type Error = ProjectionError;

    fn try_from(fields: PerspectiveFields) -> Result<Perspective, ProjectionError> {
        Perspective::new(fields.vertical_fov_radians, fields.near, fields.far)
    }
}

/// The serialized form of [`Orthographic`], with named fields.
#[derive(Deserialize)]
#[serde(rename = "Orthographic", deny_unknown_fields)]
struct OrthographicFields {
    visible_height: f32,
    near: f32,
    far: f32,
}

impl TryFrom<OrthographicFields> for Orthographic {
    type Error = ProjectionError;

    fn try_from(fields: OrthographicFields) -> Result<Orthographic, ProjectionError> {
        Orthographic::new(fields.visible_height, fields.near, fields.far)
    }
}

/// The scene's active camera: the object it looks from and the projection
/// it looks through (ADR 0024).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveCamera {
    object: ObjectHandle,
    projection: Projection,
}

impl ActiveCamera {
    pub(crate) fn new(object: ObjectHandle, projection: Projection) -> ActiveCamera {
        ActiveCamera { object, projection }
    }

    /// The object the camera looks from.
    pub fn object(&self) -> ObjectHandle {
        self.object
    }

    /// How the view maps onto the screen.
    pub fn projection(&self) -> Projection {
        self.projection
    }
}

/// Why the active camera could not be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActiveCameraError {
    /// The camera object is gone, or hasn't spawned yet.
    Gone,
}

impl fmt::Display for ActiveCameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActiveCameraError::Gone => write!(f, "the camera object is not in the scene"),
        }
    }
}

impl Error for ActiveCameraError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_rejects_values_that_break_the_math() {
        assert_eq!(
            Perspective::new(f32::NAN, 0.1, 100.0),
            Err(ProjectionError::NotFinite)
        );
        assert_eq!(
            Perspective::new(1.0, 0.1, f32::INFINITY),
            Err(ProjectionError::NotFinite)
        );
        for fov in [0.0, -1.0, PI, 4.0] {
            assert_eq!(
                Perspective::new(fov, 0.1, 100.0),
                Err(ProjectionError::FieldOfViewOutOfRange),
                "{fov}"
            );
        }
        assert_eq!(
            Perspective::new(1.0, 0.0, 100.0),
            Err(ProjectionError::NearNotPositive)
        );
        assert_eq!(
            Perspective::new(1.0, -1.0, 100.0),
            Err(ProjectionError::NearNotPositive)
        );
        assert_eq!(
            Perspective::new(1.0, 10.0, 10.0),
            Err(ProjectionError::FarNotPastNear)
        );
        assert_eq!(
            // So narrow that the zoom overflows.
            Perspective::new(1e-39, 0.1, 100.0),
            Err(ProjectionError::NotFinite)
        );
    }

    #[test]
    fn perspective_keeps_its_values() {
        let perspective = Perspective::new(1.0, 0.1, 100.0).unwrap();
        assert_eq!(perspective.vertical_fov_radians(), 1.0);
        assert_eq!(perspective.near(), 0.1);
        assert_eq!(perspective.far(), 100.0);
    }

    #[test]
    fn orthographic_rejects_values_that_break_the_math() {
        assert_eq!(
            Orthographic::new(f32::NAN, 0.0, 1.0),
            Err(ProjectionError::NotFinite)
        );
        assert_eq!(
            Orthographic::new(0.0, 0.0, 1.0),
            Err(ProjectionError::VisibleHeightNotPositive)
        );
        assert_eq!(
            Orthographic::new(10.0, 1.0, 1.0),
            Err(ProjectionError::FarNotPastNear)
        );
    }

    #[test]
    fn orthographic_allows_a_near_plane_behind_the_camera() {
        let orthographic = Orthographic::new(10.0, -5.0, 5.0).unwrap();
        assert_eq!(orthographic.near(), -5.0);
    }
}
