use std::fmt;

use bytemuck::NoUninit;

use super::Vector3;

/// A 4x4 matrix that places things in space: a scale, then a rotation, then
/// a move, possibly combined through a hierarchy.
///
/// Matrices are built from a [`Transform`](super::Transform) and combined
/// with [`Matrix4::then`]. Unlike a transform, a combined matrix can
/// represent the skew a rotated child gets under a non-uniformly scaled
/// parent, so it is exact where a position, rotation, and scale can't be
/// (ADR 0013).
///
/// Every `Matrix4` is affine: its bottom row is `0, 0, 0, 1`. SFEG has no
/// constructor for other matrices, such as camera projections, which will
/// get their own type. Like the vectors, a matrix may hold NaN or infinity
/// after math overflows (ADR 0023).
///
/// It can be cast to bytes for the GPU, in column-major order, but not
/// created from bytes, since arbitrary bytes may not be affine.
#[derive(Clone, Copy, PartialEq, NoUninit)]
#[repr(transparent)]
pub struct Matrix4(glam::Mat4);

impl Matrix4 {
    /// Leaves everything where it is.
    pub const IDENTITY: Matrix4 = Matrix4(glam::Mat4::IDENTITY);

    /// Combines two matrices: `self.then(next)` applies `self` first, then
    /// `next`.
    ///
    /// A child's world matrix is its local matrix followed by its parent's
    /// world matrix: `child_local.then(parent_world)`.
    pub fn then(self, next: Matrix4) -> Matrix4 {
        Matrix4(next.0 * self.0)
    }

    /// The matrix that undoes this one, or `None` if there isn't one.
    ///
    /// There is none when a scale of zero flattens space, or when the
    /// matrix holds non-finite values or is so close to flat that the
    /// inverse overflows.
    pub fn inverse(self) -> Option<Matrix4> {
        if self.0.determinant() == 0.0 {
            return None;
        }
        let inverse = self.0.inverse();
        inverse.is_finite().then_some(Matrix4(inverse))
    }

    /// Moves, rotates, and scales a position.
    pub fn transform_point(self, point: Vector3) -> Vector3 {
        Vector3::from_glam(self.0.transform_point3(point.to_glam()))
    }

    /// Rotates and scales an offset or direction, without moving it.
    ///
    /// A direction may come out with a length other than 1 if the matrix
    /// scales.
    pub fn transform_vector(self, vector: Vector3) -> Vector3 {
        Vector3::from_glam(self.0.transform_vector3(vector.to_glam()))
    }

    /// Where this matrix moves the origin to.
    pub fn translation(self) -> Vector3 {
        self.transform_point(Vector3::ZERO)
    }

    /// Whether every element is finite (not NaN or infinite).
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }

    /// The elements in column-major order, as the GPU expects.
    pub fn to_cols_array(self) -> [f32; 16] {
        self.0.to_cols_array()
    }

    pub(crate) fn from_glam(m: glam::Mat4) -> Matrix4 {
        Matrix4(m)
    }
}

/// Shows rows, as matrices are written on paper.
impl fmt::Debug for Matrix4 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rows = self.0.transpose().to_cols_array_2d();
        f.debug_tuple("Matrix4")
            .field(&rows[0])
            .field(&rows[1])
            .field(&rows[2])
            .field(&rows[3])
            .finish()
    }
}
