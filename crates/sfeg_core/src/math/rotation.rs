use std::error::Error;
use std::fmt;

use bytemuck::NoUninit;
use serde::{Deserialize, Serialize};

use super::Vector3;

/// A 3D rotation, stored as a unit quaternion.
///
/// A `Rotation` is always normalized and finite. Every way of creating one
/// either can't produce an invalid value or returns a [`RotationError`].
/// Nothing silently fixes a bad input; where a correction is wanted, the
/// constructor names it, as in [`Rotation::from_quat_normalized`]
/// (ADR 0010).
///
/// It can be cast to bytes for the GPU, but not created from bytes, since
/// arbitrary bytes could break the guarantee. For the same reason it has no
/// all-zero value.
///
/// In data files it is written as a quaternion with named fields:
/// `(x: 0.0, y: 0.0, z: 0.0, w: 1.0)`. Loading checks it like
/// [`Rotation::from_quat`].
#[derive(Clone, Copy, NoUninit, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(try_from = "RotationFields", into = "RotationFields")]
pub struct Rotation(glam::Quat);

impl Rotation {
    /// No rotation.
    pub const IDENTITY: Rotation = Rotation(glam::Quat::IDENTITY);

    /// How far a squared length may be from 1 and still count as normalized.
    ///
    /// f32 can rarely store a length of exactly 1, so values that are
    /// mathematically normalized, or typed by hand like `0.7071`, are slightly
    /// off. A value inside the tolerance is kept as given, never adjusted.
    /// Applies to quaternions and rotation axes.
    pub const NORMALIZED_TOLERANCE: f32 = 2e-4;

    /// Creates a rotation from quaternion components, which must already be
    /// normalized (within [`Rotation::NORMALIZED_TOLERANCE`]).
    ///
    /// # Errors
    ///
    /// - [`RotationError::NotFinite`] if any component is NaN or infinite.
    /// - [`RotationError::NotNormalized`] if the quaternion's length isn't 1.
    pub fn from_quat(x: f32, y: f32, z: f32, w: f32) -> Result<Rotation, RotationError> {
        let quat = glam::Quat::from_xyzw(x, y, z, w);
        if !quat.is_finite() {
            return Err(RotationError::NotFinite);
        }
        let length_squared = quat.length_squared();
        if !is_normalized(length_squared) {
            return Err(RotationError::NotNormalized { length_squared });
        }
        Ok(Rotation(quat))
    }

    /// Creates a rotation from quaternion components of any length,
    /// scaling them to length 1.
    ///
    /// # Errors
    ///
    /// - [`RotationError::NotFinite`] if any component is NaN or infinite.
    /// - [`RotationError::ZeroLength`] if the quaternion's length is zero or
    ///   too close to zero to scale.
    pub fn from_quat_normalized(x: f32, y: f32, z: f32, w: f32) -> Result<Rotation, RotationError> {
        let components = glam::Vec4::new(x, y, z, w);
        if !components.is_finite() {
            return Err(RotationError::NotFinite);
        }
        let normalized = components
            .try_normalize()
            .ok_or(RotationError::ZeroLength)?;
        Ok(Rotation(glam::Quat::from_vec4(normalized)))
    }

    /// A rotation of `radians` around `axis`, which must already be
    /// normalized (within [`Rotation::NORMALIZED_TOLERANCE`]).
    ///
    /// Positive angles turn counterclockwise when looking down the axis
    /// toward the origin (right-hand rule).
    ///
    /// # Errors
    ///
    /// - [`RotationError::NotFinite`] if the axis or angle is NaN or infinite.
    /// - [`RotationError::AxisNotNormalized`] if the axis's length isn't 1.
    pub fn from_axis_angle(axis: Vector3, radians: f32) -> Result<Rotation, RotationError> {
        if !axis.is_finite() || !radians.is_finite() {
            return Err(RotationError::NotFinite);
        }
        let length_squared = axis.length_squared();
        if !is_normalized(length_squared) {
            return Err(RotationError::AxisNotNormalized { length_squared });
        }
        Ok(Rotation(glam::Quat::from_axis_angle(
            axis.to_glam(),
            radians,
        )))
    }

    /// A rotation of `radians` around the x axis.
    ///
    /// # Errors
    ///
    /// [`RotationError::NotFinite`] if `radians` is NaN or infinite.
    pub fn from_rotation_x(radians: f32) -> Result<Rotation, RotationError> {
        Rotation::from_axis_angle(Vector3::UNIT_X, radians)
    }

    /// A rotation of `radians` around the y axis.
    ///
    /// # Errors
    ///
    /// [`RotationError::NotFinite`] if `radians` is NaN or infinite.
    pub fn from_rotation_y(radians: f32) -> Result<Rotation, RotationError> {
        Rotation::from_axis_angle(Vector3::UNIT_Y, radians)
    }

    /// A rotation of `radians` around the z axis. In 2D, this turns
    /// counterclockwise from +x toward +y.
    ///
    /// # Errors
    ///
    /// [`RotationError::NotFinite`] if `radians` is NaN or infinite.
    pub fn from_rotation_z(radians: f32) -> Result<Rotation, RotationError> {
        Rotation::from_axis_angle(Vector3::UNIT_Z, radians)
    }

    /// The quaternion components as `[x, y, z, w]`.
    pub fn to_array(self) -> [f32; 4] {
        self.0.to_array()
    }

    /// The rotation that undoes this one.
    pub fn inverse(self) -> Rotation {
        // The conjugate of a unit quaternion is its inverse and is also unit.
        Rotation(self.0.conjugate())
    }

    /// Rotates `vector` around the origin.
    ///
    /// Positions and offsets rotate the same way around the origin, so this
    /// works for both.
    pub fn rotate(self, vector: Vector3) -> Vector3 {
        Vector3::from_glam(self.0 * vector.to_glam())
    }

    /// Combines two rotations: `self.then(next)` applies `self` first, then
    /// `next`.
    ///
    /// Combining unit quaternions gives a unit quaternion, but f32 rounding
    /// makes the length drift over many combinations. The result is scaled
    /// back to length 1 so the guarantee keeps holding. This keeps the
    /// type's own guarantee and never changes a value the caller gave.
    pub fn then(self, next: Rotation) -> Rotation {
        Rotation((next.0 * self.0).normalize())
    }

    /// Spherical interpolation: `self` at `t = 0.0`, `end` at `t = 1.0`,
    /// turning at a constant speed along the shorter way around.
    ///
    /// `t` is not clamped; values outside `0.0..=1.0` keep turning past
    /// either end. Like [`Rotation::then`], the result is scaled back to
    /// length 1 against f32 rounding.
    ///
    /// # Errors
    ///
    /// [`RotationError::NotFinite`] if `t` is NaN or infinite.
    pub fn slerp(self, end: Rotation, t: f32) -> Result<Rotation, RotationError> {
        if !t.is_finite() {
            return Err(RotationError::NotFinite);
        }
        Ok(Rotation(self.0.slerp(end.0, t).normalize()))
    }

    pub(crate) fn to_glam(self) -> glam::Quat {
        self.0
    }
}

fn is_normalized(length_squared: f32) -> bool {
    (length_squared - 1.0).abs() <= Rotation::NORMALIZED_TOLERANCE
}

/// Two rotations are equal when they turn things the same way.
///
/// A quaternion and its negation describe the same rotation, so both
/// compare equal. Components are otherwise compared exactly.
impl PartialEq for Rotation {
    fn eq(&self, other: &Rotation) -> bool {
        self.0 == other.0 || self.0 == -other.0
    }
}

impl fmt::Debug for Rotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rotation")
            .field("x", &self.0.x)
            .field("y", &self.0.y)
            .field("z", &self.0.z)
            .field("w", &self.0.w)
            .finish()
    }
}

/// Why a [`Rotation`] could not be created.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum RotationError {
    /// A component, axis, or angle was NaN or infinite.
    NotFinite,
    /// The quaternion's length wasn't 1.
    NotNormalized {
        /// The quaternion's squared length.
        length_squared: f32,
    },
    /// The quaternion's length was zero or too close to zero to scale.
    ZeroLength,
    /// The rotation axis's length wasn't 1.
    AxisNotNormalized {
        /// The axis's squared length.
        length_squared: f32,
    },
}

impl fmt::Display for RotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RotationError::NotFinite => write!(f, "rotation values must be finite"),
            RotationError::NotNormalized { length_squared } => write!(
                f,
                "rotation quaternion must have length 1, but its squared length is {length_squared}"
            ),
            RotationError::ZeroLength => {
                write!(
                    f,
                    "rotation quaternion has zero length and can't be normalized"
                )
            }
            RotationError::AxisNotNormalized { length_squared } => write!(
                f,
                "rotation axis must have length 1, but its squared length is {length_squared}"
            ),
        }
    }
}

impl Error for RotationError {}

/// The serialized form of [`Rotation`], with named fields.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename = "Rotation", deny_unknown_fields)]
struct RotationFields {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

impl TryFrom<RotationFields> for Rotation {
    type Error = RotationError;

    fn try_from(q: RotationFields) -> Result<Rotation, RotationError> {
        Rotation::from_quat(q.x, q.y, q.z, q.w)
    }
}

impl From<Rotation> for RotationFields {
    fn from(r: Rotation) -> RotationFields {
        let [x, y, z, w] = r.to_array();
        RotationFields { x, y, z, w }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use super::*;
    use crate::math::assert_close;

    fn assert_vector_close(a: Vector3, b: Vector3) {
        assert_close(a.x(), b.x());
        assert_close(a.y(), b.y());
        assert_close(a.z(), b.z());
    }

    #[test]
    #[expect(
        clippy::approx_constant,
        reason = "tests a rounded value as a person would type it in a data file"
    )]
    fn from_quat_accepts_only_normalized_finite_values() {
        assert_eq!(
            Rotation::from_quat(0.0, 0.0, 0.0, 1.0),
            Ok(Rotation::IDENTITY)
        );
        // Hand-written values within the tolerance are accepted as given.
        let r = Rotation::from_quat(0.0, 0.0, 0.7071, 0.7071).unwrap();
        assert_eq!(r.to_array(), [0.0, 0.0, 0.7071, 0.7071]);

        assert_eq!(
            Rotation::from_quat(0.0, 0.0, 0.0, 2.0),
            Err(RotationError::NotNormalized {
                length_squared: 4.0
            })
        );
        assert_eq!(
            Rotation::from_quat(f32::NAN, 0.0, 0.0, 1.0),
            Err(RotationError::NotFinite)
        );
        assert_eq!(
            Rotation::from_quat(0.0, 0.0, 0.0, 0.0),
            Err(RotationError::NotNormalized {
                length_squared: 0.0
            })
        );
    }

    #[test]
    fn from_quat_normalized_names_the_correction() {
        assert_eq!(
            Rotation::from_quat_normalized(0.0, 0.0, 0.0, 2.0),
            Ok(Rotation::IDENTITY)
        );
        assert_eq!(
            Rotation::from_quat_normalized(0.0, 0.0, 0.0, 0.0),
            Err(RotationError::ZeroLength)
        );
        assert_eq!(
            Rotation::from_quat_normalized(f32::INFINITY, 0.0, 0.0, 1.0),
            Err(RotationError::NotFinite)
        );
    }

    #[test]
    fn from_axis_angle_requires_a_unit_axis() {
        assert_eq!(
            Rotation::from_axis_angle(Vector3::new(0.0, 0.0, 2.0), 1.0),
            Err(RotationError::AxisNotNormalized {
                length_squared: 4.0
            })
        );
        assert_eq!(
            Rotation::from_axis_angle(Vector3::UNIT_Z, f32::NAN),
            Err(RotationError::NotFinite)
        );
        assert_eq!(
            Rotation::from_rotation_z(f32::INFINITY),
            Err(RotationError::NotFinite)
        );
    }

    #[test]
    fn rotation_z_turns_counterclockwise() {
        let quarter = Rotation::from_rotation_z(FRAC_PI_2).unwrap();
        assert_vector_close(quarter.rotate(Vector3::UNIT_X), Vector3::UNIT_Y);
        assert_vector_close(
            Rotation::from_rotation_x(FRAC_PI_2)
                .unwrap()
                .rotate(Vector3::UNIT_Y),
            Vector3::UNIT_Z,
        );
        assert_vector_close(
            Rotation::from_rotation_y(FRAC_PI_2)
                .unwrap()
                .rotate(Vector3::UNIT_Z),
            Vector3::UNIT_X,
        );
    }

    #[test]
    fn then_applies_self_first() {
        let z = Rotation::from_rotation_z(FRAC_PI_2).unwrap();
        let x = Rotation::from_rotation_x(FRAC_PI_2).unwrap();
        // +x turns to +y around z, then +y turns to +z around x.
        assert_vector_close(z.then(x).rotate(Vector3::UNIT_X), Vector3::UNIT_Z);
        // The other order: +x is unchanged around x, then turns to +y.
        assert_vector_close(x.then(z).rotate(Vector3::UNIT_X), Vector3::UNIT_Y);
    }

    #[test]
    fn inverse_undoes() {
        let r = Rotation::from_axis_angle(Vector3::new(0.6, 0.8, 0.0), 1.234).unwrap();
        let v = Vector3::new(1.0, 2.0, 3.0);
        assert_vector_close(r.inverse().rotate(r.rotate(v)), v);
    }

    #[test]
    fn many_combinations_stay_normalized() {
        let step = Rotation::from_axis_angle(Vector3::new(0.6, 0.0, 0.8), 0.001).unwrap();
        let mut r = Rotation::IDENTITY;
        for _ in 0..100_000 {
            r = r.then(step);
        }
        let [x, y, z, w] = r.to_array();
        assert!(Rotation::from_quat(x, y, z, w).is_ok());
    }

    #[test]
    fn slerp_turns_at_constant_speed() {
        let quarter = Rotation::from_rotation_z(FRAC_PI_2).unwrap();
        assert_eq!(
            Rotation::IDENTITY.slerp(quarter, 0.0),
            Ok(Rotation::IDENTITY)
        );
        let halfway = Rotation::IDENTITY.slerp(quarter, 0.5).unwrap();
        let expected = Rotation::from_rotation_z(FRAC_PI_2 / 2.0).unwrap();
        assert_vector_close(
            halfway.rotate(Vector3::UNIT_X),
            expected.rotate(Vector3::UNIT_X),
        );
        assert_eq!(
            Rotation::IDENTITY.slerp(quarter, f32::NAN),
            Err(RotationError::NotFinite)
        );
    }

    #[test]
    fn slerp_takes_the_shorter_way() {
        // A turn of 3/4 around z is the same rotation as -1/4, so halfway
        // there is -1/8, not 3/8.
        let three_quarters = Rotation::from_rotation_z(3.0 * FRAC_PI_2).unwrap();
        let halfway = Rotation::IDENTITY.slerp(three_quarters, 0.5).unwrap();
        let expected = Rotation::from_rotation_z(-FRAC_PI_2 / 2.0).unwrap();
        assert_vector_close(
            halfway.rotate(Vector3::UNIT_X),
            expected.rotate(Vector3::UNIT_X),
        );
    }

    #[test]
    fn negated_quaternion_is_the_same_rotation() {
        let r = Rotation::from_rotation_z(1.0).unwrap();
        let [x, y, z, w] = r.to_array();
        assert_eq!(Rotation::from_quat(-x, -y, -z, -w).unwrap(), r);
        // A full turn is the negated identity quaternion.
        let full_turn = Rotation::from_rotation_z(2.0 * PI).unwrap();
        assert_close(full_turn.to_array()[3], -1.0);
        assert_ne!(Rotation::from_rotation_z(PI).unwrap(), Rotation::IDENTITY);
    }

    #[test]
    fn serializes_with_named_fields_and_checks_on_load() {
        let text = ron::to_string(&Rotation::IDENTITY).unwrap();
        assert_eq!(text, "(x:0.0,y:0.0,z:0.0,w:1.0)");
        assert_eq!(
            ron::from_str::<Rotation>(&text).unwrap(),
            Rotation::IDENTITY
        );

        let error = ron::from_str::<Rotation>("(x: 0.0, y: 0.0, z: 0.0, w: 2.0)").unwrap_err();
        assert!(error.to_string().contains("must have length 1"), "{error}");
    }

    #[test]
    fn casts_to_bytes_for_the_gpu() {
        let rotations = [Rotation::IDENTITY];
        let floats: &[f32] = bytemuck::cast_slice(&rotations);
        assert_eq!(floats, [0.0, 0.0, 0.0, 1.0]);
    }
}
