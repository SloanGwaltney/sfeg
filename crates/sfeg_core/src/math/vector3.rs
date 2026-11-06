use std::fmt;

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

/// A 3D vector, used for both positions and offsets (ADR 0023).
///
/// It may hold NaN or infinity, and its arithmetic doesn't check for them.
/// The engine rejects non-finite values where it stores them.
///
/// In data files it is written with named fields: `(x: 1.0, y: 2.0, z: 3.0)`.
#[derive(Clone, Copy, PartialEq, Pod, Zeroable, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(from = "Vector3Fields", into = "Vector3Fields")]
pub struct Vector3(glam::Vec3);

impl_vector_ops!(Vector3);

impl Vector3 {
    /// All components zero.
    pub const ZERO: Vector3 = Vector3::new(0.0, 0.0, 0.0);
    /// All components one.
    pub const ONE: Vector3 = Vector3::new(1.0, 1.0, 1.0);
    /// The unit vector along +x.
    pub const UNIT_X: Vector3 = Vector3::new(1.0, 0.0, 0.0);
    /// The unit vector along +y.
    pub const UNIT_Y: Vector3 = Vector3::new(0.0, 1.0, 0.0);
    /// The unit vector along +z.
    pub const UNIT_Z: Vector3 = Vector3::new(0.0, 0.0, 1.0);

    /// Creates a vector from its components.
    pub const fn new(x: f32, y: f32, z: f32) -> Vector3 {
        Vector3(glam::Vec3::new(x, y, z))
    }

    /// The x component.
    pub fn x(self) -> f32 {
        self.0.x
    }

    /// The y component.
    pub fn y(self) -> f32 {
        self.0.y
    }

    /// The z component.
    pub fn z(self) -> f32 {
        self.0.z
    }

    /// A copy with the x component replaced.
    pub fn with_x(self, x: f32) -> Vector3 {
        Vector3(self.0.with_x(x))
    }

    /// A copy with the y component replaced.
    pub fn with_y(self, y: f32) -> Vector3 {
        Vector3(self.0.with_y(y))
    }

    /// A copy with the z component replaced.
    pub fn with_z(self, z: f32) -> Vector3 {
        Vector3(self.0.with_z(z))
    }

    /// The components as `[x, y, z]`.
    pub fn to_array(self) -> [f32; 3] {
        self.0.to_array()
    }

    /// Whether every component is finite (not NaN or infinite).
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// The dot product.
    pub fn dot(self, rhs: Vector3) -> f32 {
        self.0.dot(rhs.0)
    }

    /// The cross product, following the right-hand rule.
    pub fn cross(self, rhs: Vector3) -> Vector3 {
        Vector3(self.0.cross(rhs.0))
    }

    /// The length.
    pub fn length(self) -> f32 {
        self.0.length()
    }

    /// The squared length, which is cheaper than [`Vector3::length`].
    pub fn length_squared(self) -> f32 {
        self.0.length_squared()
    }

    /// The distance between two positions.
    pub fn distance(self, rhs: Vector3) -> f32 {
        self.0.distance(rhs.0)
    }

    /// This vector scaled to length 1, or `None` if its length is zero,
    /// too close to zero, or not finite.
    pub fn try_normalize(self) -> Option<Vector3> {
        self.0.try_normalize().map(Vector3)
    }

    /// Linear interpolation: `self` at `t = 0.0`, `rhs` at `t = 1.0`.
    /// `t` is not clamped.
    pub fn lerp(self, rhs: Vector3, t: f32) -> Vector3 {
        Vector3(self.0.lerp(rhs.0, t))
    }

    pub(crate) fn to_glam(self) -> glam::Vec3 {
        self.0
    }

    pub(crate) fn from_glam(v: glam::Vec3) -> Vector3 {
        Vector3(v)
    }
}

impl fmt::Debug for Vector3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vector3")
            .field("x", &self.0.x)
            .field("y", &self.0.y)
            .field("z", &self.0.z)
            .finish()
    }
}

/// The serialized form of [`Vector3`], with named fields.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename = "Vector3", deny_unknown_fields)]
struct Vector3Fields {
    x: f32,
    y: f32,
    z: f32,
}

impl From<Vector3Fields> for Vector3 {
    fn from(v: Vector3Fields) -> Vector3 {
        Vector3::new(v.x, v.y, v.z)
    }
}

impl From<Vector3> for Vector3Fields {
    fn from(v: Vector3) -> Vector3Fields {
        Vector3Fields {
            x: v.x(),
            y: v.y(),
            z: v.z(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        let a = Vector3::new(1.0, 2.0, 3.0);
        let b = Vector3::new(4.0, 5.0, 6.0);
        assert_eq!(a + b, Vector3::new(5.0, 7.0, 9.0));
        assert_eq!(b - a, Vector3::new(3.0, 3.0, 3.0));
        assert_eq!(-a, Vector3::new(-1.0, -2.0, -3.0));
        assert_eq!(a * 2.0, Vector3::new(2.0, 4.0, 6.0));
        assert_eq!(2.0 * a, a * 2.0);
        assert_eq!(b / 2.0, Vector3::new(2.0, 2.5, 3.0));

        let mut c = a;
        c += b;
        c -= a;
        c *= 2.0;
        c /= 4.0;
        assert_eq!(c, Vector3::new(2.0, 2.5, 3.0));
    }

    #[test]
    fn products_and_lengths() {
        assert_eq!(Vector3::UNIT_X.cross(Vector3::UNIT_Y), Vector3::UNIT_Z);
        assert_eq!(Vector3::new(1.0, 2.0, 3.0).dot(Vector3::ONE), 6.0);
        assert_eq!(Vector3::new(3.0, 4.0, 0.0).length(), 5.0);
        assert_eq!(Vector3::new(3.0, 4.0, 0.0).length_squared(), 25.0);
        assert_eq!(Vector3::ZERO.distance(Vector3::new(0.0, 3.0, 4.0)), 5.0);
    }

    #[test]
    fn try_normalize() {
        assert_eq!(
            Vector3::new(0.0, 0.0, 2.0).try_normalize(),
            Some(Vector3::UNIT_Z)
        );
        assert_eq!(Vector3::ZERO.try_normalize(), None);
        assert_eq!(Vector3::new(f32::NAN, 0.0, 0.0).try_normalize(), None);
    }

    #[test]
    fn non_finite_values_are_allowed() {
        let v = Vector3::new(f32::MAX, 0.0, 0.0) * 2.0;
        assert!(!v.is_finite());
        assert!(!Vector3::new(f32::NAN, 0.0, 0.0).is_finite());
        assert!(Vector3::ONE.is_finite());
    }

    #[test]
    fn components() {
        let v = Vector3::new(1.0, 2.0, 3.0);
        assert_eq!((v.x(), v.y(), v.z()), (1.0, 2.0, 3.0));
        assert_eq!(
            v.with_x(9.0).with_y(8.0).with_z(7.0).to_array(),
            [9.0, 8.0, 7.0]
        );
        assert_eq!(Vector3::ZERO.lerp(Vector3::ONE, 0.5), Vector3::ONE * 0.5);
    }

    #[test]
    fn serializes_with_named_fields() {
        let v = Vector3::new(1.0, 2.5, -3.0);
        let text = ron::to_string(&v).unwrap();
        assert_eq!(text, "(x:1.0,y:2.5,z:-3.0)");
        assert_eq!(ron::from_str::<Vector3>(&text).unwrap(), v);
        assert!(ron::from_str::<Vector3>("(x: 1.0, y: 2.0)").is_err());
    }

    #[test]
    fn casts_to_bytes_for_the_gpu() {
        let vs = [Vector3::new(1.0, 2.0, 3.0), Vector3::new(4.0, 5.0, 6.0)];
        let floats: &[f32] = bytemuck::cast_slice(&vs);
        assert_eq!(floats, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn debug_hides_glam() {
        assert_eq!(
            format!("{:?}", Vector3::new(1.0, 2.0, 3.0)),
            "Vector3 { x: 1.0, y: 2.0, z: 3.0 }"
        );
    }
}
