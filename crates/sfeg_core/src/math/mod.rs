//! SFEG's math types.
//!
//! Every public math type is an SFEG type that wraps glam privately, so
//! games never see glam (ADR 0010). All math is f32 (ADR 0022).
//!
//! Plain vectors may hold NaN or infinity, and their arithmetic doesn't
//! check for it. The engine rejects non-finite values where it stores them,
//! such as in transforms. Types that carry a guarantee, such as
//! [`Rotation`], never hold an invalid value (ADR 0023).
//!
//! Positions and offsets share the vector types. Where the meaning changes
//! the result, the method name says which one is meant (ADR 0023).

#[macro_use]
mod vector_ops;
mod rotation;
mod vector2;
mod vector3;

pub use rotation::{Rotation, RotationError};
pub use vector2::Vector2;
pub use vector3::Vector3;

#[cfg(test)]
pub(crate) fn assert_close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} is not close to {b}");
}
