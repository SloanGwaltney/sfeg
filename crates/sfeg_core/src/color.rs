use std::error::Error;
use std::fmt;

/// An opaque color.
///
/// The same three numbers mean different colors in sRGB, the space color
/// pickers and hex codes use, and in linear RGB, the space lighting and
/// blending math use. Mixing them up gives colors that look plausible but
/// wrong, such as washed-out or too-dark shades. So every constructor names
/// its space, and the color is stored as linear RGB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    linear: [f32; 3],
}

impl Color {
    /// Black.
    pub const BLACK: Color = Color {
        linear: [0.0, 0.0, 0.0],
    };

    /// White.
    pub const WHITE: Color = Color {
        linear: [1.0, 1.0, 1.0],
    };

    /// A color from sRGB components from 0 to 255, as in a hex code such as
    /// `#ff8800`.
    pub fn from_srgb_u8(red: u8, green: u8, blue: u8) -> Color {
        let to_linear = |component: u8| srgb_to_linear(f32::from(component) / 255.0);
        Color {
            linear: [to_linear(red), to_linear(green), to_linear(blue)],
        }
    }

    /// A color from sRGB components from 0 to 1, as most color pickers show
    /// them.
    ///
    /// # Errors
    ///
    /// [`ColorError::OutOfRange`] if a component is below 0, above 1, or
    /// NaN.
    pub fn from_srgb(red: f32, green: f32, blue: f32) -> Result<Color, ColorError> {
        let [red, green, blue] = check_range([red, green, blue])?;
        Ok(Color {
            linear: [
                srgb_to_linear(red),
                srgb_to_linear(green),
                srgb_to_linear(blue),
            ],
        })
    }

    /// A color from linear RGB components from 0 to 1.
    ///
    /// # Errors
    ///
    /// [`ColorError::OutOfRange`] if a component is below 0, above 1, or
    /// NaN.
    pub fn from_linear(red: f32, green: f32, blue: f32) -> Result<Color, ColorError> {
        Ok(Color {
            linear: check_range([red, green, blue])?,
        })
    }

    /// The red, green, and blue components in linear RGB, from 0 to 1.
    pub fn to_linear_array(self) -> [f32; 3] {
        self.linear
    }
}

fn check_range(components: [f32; 3]) -> Result<[f32; 3], ColorError> {
    if components
        .iter()
        .all(|component| (0.0..=1.0).contains(component))
    {
        Ok(components)
    } else {
        Err(ColorError::OutOfRange)
    }
}

/// The standard sRGB transfer function, from an encoded component to a
/// linear one.
fn srgb_to_linear(component: f32) -> f32 {
    if component <= 0.04045 {
        component / 12.92
    } else {
        ((component + 0.055) / 1.055).powf(2.4)
    }
}

/// Why a [`Color`] could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColorError {
    /// A component is below 0, above 1, or NaN.
    OutOfRange,
}

impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColorError::OutOfRange => write!(f, "color components must be from 0 to 1"),
        }
    }
}

impl Error for ColorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::assert_close;

    #[test]
    fn srgb_mid_gray_is_darker_in_linear() {
        let [red, green, blue] = Color::from_srgb(0.5, 0.5, 0.5).unwrap().to_linear_array();
        assert_close(red, 0.21404);
        assert_eq!(red, green);
        assert_eq!(green, blue);
    }

    #[test]
    fn the_ends_are_the_same_in_both_spaces() {
        assert_eq!(Color::from_srgb_u8(0, 0, 0), Color::BLACK);
        assert_eq!(Color::from_srgb_u8(255, 255, 255), Color::WHITE);
        assert_eq!(Color::from_srgb(1.0, 1.0, 1.0), Ok(Color::WHITE));
        assert_eq!(Color::from_linear(1.0, 1.0, 1.0), Ok(Color::WHITE));
    }

    #[test]
    fn u8_and_f32_srgb_agree() {
        let from_u8 = Color::from_srgb_u8(255, 136, 0).to_linear_array();
        let from_f32 = Color::from_srgb(1.0, 136.0 / 255.0, 0.0)
            .unwrap()
            .to_linear_array();
        assert_eq!(from_u8, from_f32);
    }

    #[test]
    fn linear_is_stored_as_given() {
        let color = Color::from_linear(0.25, 0.5, 0.75).unwrap();
        assert_eq!(color.to_linear_array(), [0.25, 0.5, 0.75]);
    }

    #[test]
    fn out_of_range_components_are_errors() {
        for bad in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert_eq!(Color::from_srgb(bad, 0.0, 0.0), Err(ColorError::OutOfRange));
            assert_eq!(
                Color::from_linear(0.0, 0.0, bad),
                Err(ColorError::OutOfRange)
            );
        }
    }
}
