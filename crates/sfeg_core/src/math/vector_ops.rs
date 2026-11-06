//! Arithmetic shared by the vector types.
//!
//! The operators don't check for NaN or infinity and never return errors
//! (ADR 0023).

macro_rules! impl_vector_ops {
    ($vector:ident) => {
        impl ::std::ops::Add for $vector {
            type Output = $vector;
            fn add(self, rhs: $vector) -> $vector {
                $vector(self.0 + rhs.0)
            }
        }

        impl ::std::ops::AddAssign for $vector {
            fn add_assign(&mut self, rhs: $vector) {
                self.0 += rhs.0;
            }
        }

        impl ::std::ops::Sub for $vector {
            type Output = $vector;
            fn sub(self, rhs: $vector) -> $vector {
                $vector(self.0 - rhs.0)
            }
        }

        impl ::std::ops::SubAssign for $vector {
            fn sub_assign(&mut self, rhs: $vector) {
                self.0 -= rhs.0;
            }
        }

        impl ::std::ops::Neg for $vector {
            type Output = $vector;
            fn neg(self) -> $vector {
                $vector(-self.0)
            }
        }

        impl ::std::ops::Mul<f32> for $vector {
            type Output = $vector;
            fn mul(self, rhs: f32) -> $vector {
                $vector(self.0 * rhs)
            }
        }

        impl ::std::ops::Mul<$vector> for f32 {
            type Output = $vector;
            fn mul(self, rhs: $vector) -> $vector {
                $vector(self * rhs.0)
            }
        }

        impl ::std::ops::MulAssign<f32> for $vector {
            fn mul_assign(&mut self, rhs: f32) {
                self.0 *= rhs;
            }
        }

        impl ::std::ops::Div<f32> for $vector {
            type Output = $vector;
            fn div(self, rhs: f32) -> $vector {
                $vector(self.0 / rhs)
            }
        }

        impl ::std::ops::DivAssign<f32> for $vector {
            fn div_assign(&mut self, rhs: f32) {
                self.0 /= rhs;
            }
        }
    };
}
