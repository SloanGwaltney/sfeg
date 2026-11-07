use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::{Matrix4, Rotation, Vector3};

/// Where an object is, how it is turned, and how big it is (ADR 0013).
///
/// Applied to a point, the scale comes first, then the rotation, then the
/// position. Each object's transform is local: relative to its parent, or to
/// the world if it has none.
///
/// A `Transform` is always finite. It is where the engine stores positions
/// and scales, so it is the seam that rejects NaN and infinity (ADR 0023):
/// every constructor and setter that takes a vector checks it and returns a
/// [`TransformError`]. The rotation is always valid because [`Rotation`]
/// guarantees it.
///
/// Scale may be zero or negative. Zero hides an object and negative mirrors
/// it, such as flipping a 2D sprite.
///
/// In data files it is written with named fields, all required:
/// `(position: (x: 0.0, y: 0.0, z: 0.0), rotation: (x: 0.0, y: 0.0, z: 0.0, w: 1.0), scale: (x: 1.0, y: 1.0, z: 1.0))`.
/// Loading checks it like [`Transform::new`].
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(try_from = "TransformFields", into = "TransformFields")]
pub struct Transform {
    position: Vector3,
    rotation: Rotation,
    scale: Vector3,
}

impl Transform {
    /// At the origin, not rotated, at scale 1.
    pub const IDENTITY: Transform = Transform {
        position: Vector3::ZERO,
        rotation: Rotation::IDENTITY,
        scale: Vector3::ONE,
    };

    /// Creates a transform from its parts.
    ///
    /// # Errors
    ///
    /// - [`TransformError::PositionNotFinite`] if `position` has a NaN or
    ///   infinite component.
    /// - [`TransformError::ScaleNotFinite`] if `scale` does.
    pub fn new(
        position: Vector3,
        rotation: Rotation,
        scale: Vector3,
    ) -> Result<Transform, TransformError> {
        let mut transform = Transform::IDENTITY;
        transform.set_position(position)?;
        transform.set_rotation(rotation);
        transform.set_scale(scale)?;
        Ok(transform)
    }

    /// A transform at `position`, not rotated, at scale 1.
    ///
    /// # Errors
    ///
    /// [`TransformError::PositionNotFinite`] if `position` has a NaN or
    /// infinite component.
    pub fn from_position(position: Vector3) -> Result<Transform, TransformError> {
        let mut transform = Transform::IDENTITY;
        transform.set_position(position)?;
        Ok(transform)
    }

    /// The position.
    pub fn position(&self) -> Vector3 {
        self.position
    }

    /// The rotation.
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// The scale along each axis, before rotating.
    pub fn scale(&self) -> Vector3 {
        self.scale
    }

    /// Replaces the position.
    ///
    /// # Errors
    ///
    /// [`TransformError::PositionNotFinite`] if `position` has a NaN or
    /// infinite component. The transform is left unchanged.
    pub fn set_position(&mut self, position: Vector3) -> Result<(), TransformError> {
        if !position.is_finite() {
            return Err(TransformError::PositionNotFinite { position });
        }
        self.position = position;
        Ok(())
    }

    /// Replaces the rotation. It can't fail, since every [`Rotation`] is
    /// valid.
    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.rotation = rotation;
    }

    /// Replaces the scale.
    ///
    /// # Errors
    ///
    /// [`TransformError::ScaleNotFinite`] if `scale` has a NaN or infinite
    /// component. The transform is left unchanged.
    pub fn set_scale(&mut self, scale: Vector3) -> Result<(), TransformError> {
        if !scale.is_finite() {
            return Err(TransformError::ScaleNotFinite { scale });
        }
        self.scale = scale;
        Ok(())
    }

    /// Blends from `previous` at `t = 0.0` toward `current` at `t = 1.0`:
    /// position and scale linearly, rotation along the shorter arc
    /// (ADR 0013).
    ///
    /// If blending overflows to a non-finite value, the result is
    /// `current`.
    pub(crate) fn interpolate(previous: &Transform, current: &Transform, t: f32) -> Transform {
        let rotation = previous
            .rotation
            .slerp(current.rotation, t)
            .unwrap_or(current.rotation);
        Transform::new(
            previous.position.lerp(current.position, t),
            rotation,
            previous.scale.lerp(current.scale, t),
        )
        .unwrap_or(*current)
    }

    /// The matrix that scales, then rotates, then moves to the position.
    pub fn to_matrix(&self) -> Matrix4 {
        Matrix4::from_glam(glam::Mat4::from_scale_rotation_translation(
            self.scale.to_glam(),
            self.rotation.to_glam(),
            self.position.to_glam(),
        ))
    }
}

impl Default for Transform {
    /// [`Transform::IDENTITY`].
    fn default() -> Transform {
        Transform::IDENTITY
    }
}

/// Why a [`Transform`] could not be created or changed.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum TransformError {
    /// The position had a NaN or infinite component.
    PositionNotFinite {
        /// The rejected position.
        position: Vector3,
    },
    /// The scale had a NaN or infinite component.
    ScaleNotFinite {
        /// The rejected scale.
        scale: Vector3,
    },
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransformError::PositionNotFinite { position } => {
                write!(
                    f,
                    "transform position must be finite, but it is {position:?}"
                )
            }
            TransformError::ScaleNotFinite { scale } => {
                write!(f, "transform scale must be finite, but it is {scale:?}")
            }
        }
    }
}

impl Error for TransformError {}

/// The serialized form of [`Transform`], with named fields.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename = "Transform", deny_unknown_fields)]
struct TransformFields {
    position: Vector3,
    rotation: Rotation,
    scale: Vector3,
}

impl TryFrom<TransformFields> for Transform {
    type Error = TransformError;

    fn try_from(t: TransformFields) -> Result<Transform, TransformError> {
        Transform::new(t.position, t.rotation, t.scale)
    }
}

impl From<Transform> for TransformFields {
    fn from(t: Transform) -> TransformFields {
        TransformFields {
            position: t.position,
            rotation: t.rotation,
            scale: t.scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::math::assert_close;

    fn assert_vector_close(a: Vector3, b: Vector3) {
        assert_close(a.x(), b.x());
        assert_close(a.y(), b.y());
        assert_close(a.z(), b.z());
    }

    fn quarter_turn_z() -> Rotation {
        Rotation::from_rotation_z(FRAC_PI_2).unwrap()
    }

    #[test]
    fn rejects_non_finite_vectors() {
        let nan = Vector3::new(f32::NAN, 0.0, 0.0);
        let infinite = Vector3::new(0.0, f32::INFINITY, 0.0);
        assert!(matches!(
            Transform::new(nan, Rotation::IDENTITY, Vector3::ONE),
            Err(TransformError::PositionNotFinite { .. })
        ));
        assert_eq!(
            Transform::new(Vector3::ZERO, Rotation::IDENTITY, infinite),
            Err(TransformError::ScaleNotFinite { scale: infinite })
        );
        assert!(Transform::from_position(infinite).is_err());
    }

    #[test]
    fn failed_set_leaves_transform_unchanged() {
        let mut t = Transform::from_position(Vector3::ONE).unwrap();
        assert!(t.set_position(Vector3::new(f32::NAN, 0.0, 0.0)).is_err());
        assert!(t.set_scale(Vector3::new(f32::INFINITY, 1.0, 1.0)).is_err());
        assert_eq!(t, Transform::from_position(Vector3::ONE).unwrap());
    }

    #[test]
    fn allows_zero_and_negative_scale() {
        let mut t = Transform::IDENTITY;
        assert!(t.set_scale(Vector3::ZERO).is_ok());
        assert!(t.set_scale(Vector3::new(-1.0, 1.0, 1.0)).is_ok());
        assert_eq!(t.scale(), Vector3::new(-1.0, 1.0, 1.0));
    }

    #[test]
    fn matrix_scales_then_rotates_then_moves() {
        let t = Transform::new(
            Vector3::new(10.0, 0.0, 0.0),
            quarter_turn_z(),
            Vector3::new(2.0, 1.0, 1.0),
        )
        .unwrap();
        let m = t.to_matrix();
        // +x scales to (2, 0, 0), turns to (0, 2, 0), then moves.
        assert_vector_close(
            m.transform_point(Vector3::UNIT_X),
            Vector3::new(10.0, 2.0, 0.0),
        );
        // Offsets are scaled and turned but not moved.
        assert_vector_close(
            m.transform_vector(Vector3::UNIT_X),
            Vector3::new(0.0, 2.0, 0.0),
        );
        assert_vector_close(m.translation(), Vector3::new(10.0, 0.0, 0.0));
    }

    #[test]
    fn child_follows_parent() {
        let parent =
            Transform::new(Vector3::new(5.0, 0.0, 0.0), quarter_turn_z(), Vector3::ONE).unwrap();
        let child = Transform::from_position(Vector3::new(1.0, 0.0, 0.0)).unwrap();
        let child_world = child.to_matrix().then(parent.to_matrix());
        // The child's offset of +x turns to +y with the parent.
        assert_vector_close(child_world.translation(), Vector3::new(5.0, 1.0, 0.0));
    }

    #[test]
    fn matrix_is_exact_under_non_uniform_parent_scale() {
        // A rotated child under a parent stretched along x is skewed, which
        // a position, rotation, and scale can't represent. The matrix can.
        let parent = Transform::new(
            Vector3::ZERO,
            Rotation::IDENTITY,
            Vector3::new(2.0, 1.0, 1.0),
        )
        .unwrap();
        let child = Transform::new(
            Vector3::ZERO,
            Rotation::from_rotation_z(FRAC_PI_2 / 2.0).unwrap(),
            Vector3::ONE,
        )
        .unwrap();
        let world = child.to_matrix().then(parent.to_matrix());
        let x = world.transform_vector(Vector3::UNIT_X);
        let y = world.transform_vector(Vector3::UNIT_Y);
        // The child's axes are no longer at right angles.
        assert!(x.dot(y).abs() > 0.1);
    }

    #[test]
    fn inverse_undoes_and_fails_for_zero_scale() {
        let t = Transform::new(
            Vector3::new(1.0, 2.0, 3.0),
            quarter_turn_z(),
            Vector3::new(2.0, -1.0, 0.5),
        )
        .unwrap();
        let m = t.to_matrix();
        let p = Vector3::new(4.0, 5.0, 6.0);
        assert_vector_close(
            m.inverse().unwrap().transform_point(m.transform_point(p)),
            p,
        );

        let flat = Transform::new(
            Vector3::ZERO,
            Rotation::IDENTITY,
            Vector3::new(1.0, 0.0, 1.0),
        )
        .unwrap();
        assert_eq!(flat.to_matrix().inverse(), None);
    }

    #[test]
    fn identity_matrix_changes_nothing() {
        assert_eq!(Transform::IDENTITY.to_matrix(), Matrix4::IDENTITY);
        assert_eq!(Transform::default(), Transform::IDENTITY);
        let m = Transform::from_position(Vector3::ONE).unwrap().to_matrix();
        assert_eq!(m.then(Matrix4::IDENTITY), m);
        assert_eq!(Matrix4::IDENTITY.then(m), m);
    }

    #[test]
    fn matrix_casts_to_column_major_bytes() {
        let m = Transform::from_position(Vector3::new(1.0, 2.0, 3.0))
            .unwrap()
            .to_matrix();
        let floats: &[f32] = bytemuck::cast_slice(std::slice::from_ref(&m));
        assert_eq!(&floats[12..], [1.0, 2.0, 3.0, 1.0]);
        assert_eq!(floats, m.to_cols_array());
    }

    #[test]
    fn serializes_with_named_fields_and_checks_on_load() {
        let t = Transform::from_position(Vector3::new(1.0, 2.0, 3.0)).unwrap();
        let text = ron::to_string(&t).unwrap();
        assert_eq!(
            text,
            "(position:(x:1.0,y:2.0,z:3.0),rotation:(x:0.0,y:0.0,z:0.0,w:1.0),scale:(x:1.0,y:1.0,z:1.0))"
        );
        assert_eq!(ron::from_str::<Transform>(&text).unwrap(), t);

        // All fields are required.
        assert!(ron::from_str::<Transform>("(position: (x: 0.0, y: 0.0, z: 0.0))").is_err());
        // Non-finite values are rejected on load.
        let error = ron::from_str::<Transform>(
            "(position: (x: inf, y: 0.0, z: 0.0), rotation: (x: 0.0, y: 0.0, z: 0.0, w: 1.0), scale: (x: 1.0, y: 1.0, z: 1.0))",
        )
        .unwrap_err();
        assert!(error.to_string().contains("must be finite"), "{error}");
    }

    #[test]
    fn debug_shows_rows() {
        let m = Transform::from_position(Vector3::new(1.0, 2.0, 3.0))
            .unwrap()
            .to_matrix();
        assert_eq!(
            format!("{m:?}"),
            "Matrix4([1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 2.0], [0.0, 0.0, 1.0, 3.0], [0.0, 0.0, 0.0, 1.0])"
        );
    }
}
