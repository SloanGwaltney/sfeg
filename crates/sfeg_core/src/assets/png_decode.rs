//! Decodes PNG files into RGBA8 pixels (ADR 0015: textures use PNG).

use std::io::Cursor;
use std::num::NonZeroU32;

use super::{DecodedTexture, TextureLoadError};

/// The widest or tallest texture SFEG loads, in pixels. It is the largest
/// size every GPU wgpu supports by default, so a texture that decodes can
/// always be uploaded.
pub const MAX_TEXTURE_SIDE_PX: u32 = 8192;

/// Bytes in one RGBA8 pixel.
const BYTES_PER_PIXEL: usize = 4;

/// Decodes `bytes` as a PNG into 8-bit RGBA pixels, sRGB-encoded, from the
/// top row down.
///
/// Every PNG color type is accepted: grayscale and palette images are
/// expanded to RGBA, images without alpha become opaque, and 16-bit
/// channels are cut to 8 bits.
///
/// Files are untrusted input: a malformed one gives an error, never a
/// panic.
pub(crate) fn decode_png(bytes: &[u8]) -> Result<DecodedTexture, TextureLoadError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|source| TextureLoadError::DecodePng(Box::new(source)))?;

    let (width, height) = reader.info().size();
    if width > MAX_TEXTURE_SIDE_PX || height > MAX_TEXTURE_SIDE_PX {
        return Err(TextureLoadError::TooLarge {
            width_px: width,
            height_px: height,
        });
    }
    // PNG requires a non-zero size, so the decoder already rejected zero.
    let (Some(width_px), Some(height_px)) = (NonZeroU32::new(width), NonZeroU32::new(height))
    else {
        return Err(TextureLoadError::DecodePng(
            "the image has no pixels".into(),
        ));
    };

    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| TextureLoadError::DecodePng("the image is too large to decode".into()))?;
    let mut decoded = vec![0; buffer_size];
    let frame = reader
        .next_frame(&mut decoded)
        .map_err(|source| TextureLoadError::DecodePng(Box::new(source)))?;
    decoded.truncate(frame.buffer_size());

    let pixel_count = width as usize * height as usize;
    let rgba8 = match frame.color_type {
        png::ColorType::Rgba => decoded,
        png::ColorType::Rgb => decoded
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, u8::MAX])
            .collect(),
        png::ColorType::GrayscaleAlpha => decoded
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
            .collect(),
        png::ColorType::Grayscale => decoded
            .iter()
            .flat_map(|&gray| [gray, gray, gray, u8::MAX])
            .collect(),
        // Expanding turns palettes into RGB or RGBA.
        png::ColorType::Indexed => {
            return Err(TextureLoadError::DecodePng(
                "the palette image wasn't expanded".into(),
            ));
        }
    };
    if rgba8.len() != pixel_count * BYTES_PER_PIXEL {
        return Err(TextureLoadError::DecodePng(
            "the decoded image has the wrong number of pixels".into(),
        ));
    }
    Ok(DecodedTexture {
        width_px,
        height_px,
        rgba8: rgba8.into(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Encodes `pixels` as a PNG of the given color type and size.
    pub(crate) fn encode_png(
        width: u32,
        height: u32,
        color: png::ColorType,
        pixels: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
        writer.finish().unwrap();
        bytes
    }

    #[test]
    fn rgba_is_kept_as_is() {
        let pixels = [255, 0, 0, 255, 0, 255, 0, 128];
        let decoded = decode_png(&encode_png(2, 1, png::ColorType::Rgba, &pixels)).unwrap();
        assert_eq!(decoded.width_px().get(), 2);
        assert_eq!(decoded.height_px().get(), 1);
        assert_eq!(decoded.rgba8(), &pixels);
    }

    #[test]
    fn rgb_becomes_opaque_rgba() {
        let pixels = [10, 20, 30, 40, 50, 60];
        let decoded = decode_png(&encode_png(1, 2, png::ColorType::Rgb, &pixels)).unwrap();
        assert_eq!(decoded.rgba8(), &[10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn grayscale_is_expanded() {
        let gray = decode_png(&encode_png(1, 1, png::ColorType::Grayscale, &[7])).unwrap();
        assert_eq!(gray.rgba8(), &[7, 7, 7, 255]);
        let gray_alpha =
            decode_png(&encode_png(1, 1, png::ColorType::GrayscaleAlpha, &[7, 9])).unwrap();
        assert_eq!(gray_alpha.rgba8(), &[7, 7, 7, 9]);
    }

    #[test]
    fn malformed_files_are_errors() {
        assert!(matches!(
            decode_png(b"not a png"),
            Err(TextureLoadError::DecodePng(_))
        ));
        let mut cut_short = encode_png(4, 4, png::ColorType::Rgba, &[200; 64]);
        cut_short.truncate(cut_short.len() - 20);
        assert!(matches!(
            decode_png(&cut_short),
            Err(TextureLoadError::DecodePng(_))
        ));
    }

    #[test]
    fn textures_wider_than_the_limit_are_rejected() {
        let width = MAX_TEXTURE_SIDE_PX + 1;
        let bytes = encode_png(
            width,
            1,
            png::ColorType::Grayscale,
            &vec![0; width as usize],
        );
        assert!(matches!(
            decode_png(&bytes),
            Err(TextureLoadError::TooLarge { width_px, height_px: 1 }) if width_px == width
        ));
    }
}
