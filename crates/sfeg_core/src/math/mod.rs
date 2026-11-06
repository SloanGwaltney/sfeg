//! SFEG's math types.
//!
//! Every public math type is an SFEG type that wraps glam privately, so
//! games never see glam (ADR 0010). All math is f32 (ADR 0022).
//!
//! Plain vectors may hold NaN or infinity, and their arithmetic doesn't
//! check for it. The engine rejects non-finite values where it stores them,
//! such as in transforms. Types that carry a guarantee, such as
//! `Rotation`, never hold an invalid value (ADR 0023).
//!
//! Positions and offsets share the vector types. Where the meaning changes
//! the result, the method name says which one is meant (ADR 0023).

#[macro_use]
mod vector_ops;
mod vector2;
mod vector3;

pub use vector2::Vector2;
pub use vector3::Vector3;

/// How far a squared length may be from 1 and still count as normalized.
///
/// This allows for f32 rounding, such as in values written by hand like
/// `0.7071`. A value inside the tolerance is kept as given, never adjusted.
pub const NORMALIZED_TOLERANCE: f32 = 2e-4;

fn is_normalized(length_squared: f32) -> bool {
    (length_squared - 1.0).abs() <= NORMALIZED_TOLERANCE
}

#[cfg(test)]
pub(crate) fn assert_close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} is not close to {b}");
}
