use std::fmt;

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

/// A 2D vector, used for both positions and offsets (ADR 0023).
///
/// It may hold NaN or infinity, and its arithmetic doesn't check for them.
/// The engine rejects non-finite values where it stores them.
///
/// In data files it is written with named fields: `(x: 1.0, y: 2.0)`.
#[derive(Clone, Copy, PartialEq, Pod, Zeroable, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(from = "Vector2Fields", into = "Vector2Fields")]
pub struct Vector2(glam::Vec2);

impl_vector_ops!(Vector2);

impl Vector2 {
    /// All components zero.
    pub const ZERO: Vector2 = Vector2::new(0.0, 0.0);
    /// All components one.
    pub const ONE: Vector2 = Vector2::new(1.0, 1.0);
    /// The unit vector along +x.
    pub const UNIT_X: Vector2 = Vector2::new(1.0, 0.0);
    /// The unit vector along +y.
    pub const UNIT_Y: Vector2 = Vector2::new(0.0, 1.0);

    /// Creates a vector from its components.
    pub const fn new(x: f32, y: f32) -> Vector2 {
        Vector2(glam::Vec2::new(x, y))
    }

    /// The x component.
    pub fn x(self) -> f32 {
        self.0.x
    }

    /// The y component.
    pub fn y(self) -> f32 {
        self.0.y
    }

    /// A copy with the x component replaced.
    pub fn with_x(self, x: f32) -> Vector2 {
        Vector2(self.0.with_x(x))
    }

    /// A copy with the y component replaced.
    pub fn with_y(self, y: f32) -> Vector2 {
        Vector2(self.0.with_y(y))
    }

    /// The components as `[x, y]`.
    pub fn to_array(self) -> [f32; 2] {
        self.0.to_array()
    }

    /// Whether every component is finite (not NaN or infinite).
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// The dot product.
    pub fn dot(self, rhs: Vector2) -> f32 {
        self.0.dot(rhs.0)
    }

    /// The length.
    pub fn length(self) -> f32 {
        self.0.length()
    }

    /// The squared length, which is cheaper than [`Vector2::length`].
    pub fn length_squared(self) -> f32 {
        self.0.length_squared()
    }

    /// The distance between two positions.
    pub fn distance(self, rhs: Vector2) -> f32 {
        self.0.distance(rhs.0)
    }

    /// This vector scaled to length 1, or `None` if its length is zero,
    /// too close to zero, or not finite.
    pub fn try_normalize(self) -> Option<Vector2> {
        self.0.try_normalize().map(Vector2)
    }

    /// Linear interpolation: `self` at `t = 0.0`, `rhs` at `t = 1.0`.
    /// `t` is not clamped.
    pub fn lerp(self, rhs: Vector2, t: f32) -> Vector2 {
        Vector2(self.0.lerp(rhs.0, t))
    }
}

impl fmt::Debug for Vector2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vector2")
            .field("x", &self.0.x)
            .field("y", &self.0.y)
            .finish()
    }
}

/// The serialized form of [`Vector2`], with named fields.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename = "Vector2", deny_unknown_fields)]
struct Vector2Fields {
    x: f32,
    y: f32,
}

impl From<Vector2Fields> for Vector2 {
    fn from(v: Vector2Fields) -> Vector2 {
        Vector2::new(v.x, v.y)
    }
}

impl From<Vector2> for Vector2Fields {
    fn from(v: Vector2) -> Vector2Fields {
        Vector2Fields { x: v.x(), y: v.y() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        let a = Vector2::new(1.0, 2.0);
        let b = Vector2::new(4.0, 6.0);
        assert_eq!(a + b, Vector2::new(5.0, 8.0));
        assert_eq!(b - a, Vector2::new(3.0, 4.0));
        assert_eq!(-a, Vector2::new(-1.0, -2.0));
        assert_eq!(2.0 * a, Vector2::new(2.0, 4.0));
        assert_eq!(b / 2.0, Vector2::new(2.0, 3.0));

        let mut c = a;
        c += b;
        c -= a;
        c *= 2.0;
        c /= 4.0;
        assert_eq!(c, Vector2::new(2.0, 3.0));
    }

    #[test]
    fn products_and_lengths() {
        assert_eq!(Vector2::new(1.0, 2.0).dot(Vector2::ONE), 3.0);
        assert_eq!(Vector2::new(3.0, 4.0).length(), 5.0);
        assert_eq!(Vector2::ZERO.distance(Vector2::new(3.0, 4.0)), 5.0);
        assert_eq!(
            Vector2::new(0.0, 2.0).try_normalize(),
            Some(Vector2::UNIT_Y)
        );
        assert_eq!(Vector2::ZERO.try_normalize(), None);
    }

    #[test]
    fn components() {
        let v = Vector2::new(1.0, 2.0);
        assert_eq!((v.x(), v.y()), (1.0, 2.0));
        assert_eq!(v.with_x(9.0).with_y(8.0).to_array(), [9.0, 8.0]);
        assert!(!Vector2::new(f32::INFINITY, 0.0).is_finite());
    }

    #[test]
    fn serializes_with_named_fields() {
        let v = Vector2::new(1.0, -2.5);
        let text = ron::to_string(&v).unwrap();
        assert_eq!(text, "(x:1.0,y:-2.5)");
        assert_eq!(ron::from_str::<Vector2>(&text).unwrap(), v);
    }

    #[test]
    fn casts_to_bytes_and_debug_hides_glam() {
        let vectors = [Vector2::new(1.0, 2.0)];
        let floats: &[f32] = bytemuck::cast_slice(&vectors);
        assert_eq!(floats, [1.0, 2.0]);
        assert_eq!(
            format!("{:?}", Vector2::new(1.0, 2.0)),
            "Vector2 { x: 1.0, y: 2.0 }"
        );
    }
}
