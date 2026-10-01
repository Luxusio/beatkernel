//! Bounded static image preparation outside native callbacks and presentation.
//! Input, native decoded bytes and RGBA output are bounded separately. Decoder
//! scratch and simultaneous conversion buffers are not a process-memory sandbox.
use crate::texture::{MAX_TEXTURE_BYTES, RgbaImage};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::io::Cursor;

const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_EXTENT: u32 = 16_384;

/// Stable preparation classification used by the immutable image bank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageDecodeError {
    /// Signature or codec features are unsupported.
    Unsupported,
    /// Supported encoded data is damaged or malformed.
    InvalidData(String),
    /// Configuration, dimensions, encoded/native/output budgets were exceeded.
    Limit(String),
}
impl std::fmt::Display for ImageDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported static image format"),
            Self::InvalidData(reason) => write!(f, "invalid static image: {reason}"),
            Self::Limit(reason) => write!(f, "static image limit: {reason}"),
        }
    }
}
impl std::error::Error for ImageDecodeError {}

/// Caller-selected bounds within fixed preparation safety ceilings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageDecodeLimits {
    /// Encoded bytes, at most 64 MiB.
    pub max_encoded_bytes: usize,
    /// Source width, at most 16384 pixels; no automatic resizing.
    pub max_width: u32,
    /// Source height, at most 16384 pixels.
    pub max_height: u32,
    /// Budget for each native decoded buffer and final RGBA8 output, at most 64 MiB.
    pub max_decoded_bytes: u64,
}
impl Default for ImageDecodeLimits {
    fn default() -> Self {
        Self {
            max_encoded_bytes: MAX_ENCODED_BYTES,
            max_width: 4096,
            max_height: 4096,
            max_decoded_bytes: MAX_TEXTURE_BYTES,
        }
    }
}
impl ImageDecodeLimits {
    /// Rejects zero or out-of-ceiling configuration before codec work.
    pub fn validate(&self) -> Result<(), ImageDecodeError> {
        if !(1..=MAX_ENCODED_BYTES).contains(&self.max_encoded_bytes)
            || !(1..=MAX_EXTENT).contains(&self.max_width)
            || !(1..=MAX_EXTENT).contains(&self.max_height)
            || !(1..=MAX_TEXTURE_BYTES).contains(&self.max_decoded_bytes)
        {
            return Err(ImageDecodeError::Limit(
                "invalid decoding configuration".into(),
            ));
        }
        Ok(())
    }
}
fn codec_error(error: image::ImageError) -> ImageDecodeError {
    match error {
        image::ImageError::Limits(error) => ImageDecodeError::Limit(error.to_string()),
        image::ImageError::Unsupported(_) => ImageDecodeError::Unsupported,
        error => ImageDecodeError::InvalidData(error.to_string()),
    }
}

/// Decodes signature-selected PNG/BMP/JPEG to unscaled straight-alpha RGBA8.
/// File names, native APIs, orientation metadata and wall clocks are not used.
/// Supported codecs retain their source pixel order; EXIF orientation is not applied.
pub fn decode(encoded: &[u8], limits: ImageDecodeLimits) -> Result<RgbaImage, ImageDecodeError> {
    limits.validate()?;
    if encoded.len() > limits.max_encoded_bytes {
        return Err(ImageDecodeError::Limit(
            "encoded image bytes exceeded".into(),
        ));
    }
    let format = if encoded.starts_with(b"\x89PNG\r\n\x1a\n") {
        ImageFormat::Png
    } else if encoded.starts_with(b"BM") {
        ImageFormat::Bmp
    } else if encoded.starts_with(b"\xff\xd8\xff") {
        ImageFormat::Jpeg
    } else {
        return Err(ImageDecodeError::Unsupported);
    };
    let mut upstream = image::Limits::default();
    upstream.max_image_width = Some(limits.max_width);
    upstream.max_image_height = Some(limits.max_height);
    upstream.max_alloc = Some(limits.max_decoded_bytes);
    let mut reader = ImageReader::with_format(Cursor::new(encoded), format);
    reader.limits(upstream);
    let decoder = reader.into_decoder().map_err(codec_error)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(ImageDecodeError::InvalidData("zero image extent".into()));
    }
    if width > limits.max_width || height > limits.max_height {
        return Err(ImageDecodeError::Limit("image extent exceeded".into()));
    }
    let rgba_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| ImageDecodeError::Limit("RGBA extent overflow".into()))?;
    if rgba_bytes > limits.max_decoded_bytes
        || decoder.total_bytes() > limits.max_decoded_bytes
        || usize::try_from(rgba_bytes).is_err()
        || usize::try_from(decoder.total_bytes()).is_err()
    {
        return Err(ImageDecodeError::Limit(
            "decoded image bytes exceeded".into(),
        ));
    }
    let rgba = DynamicImage::from_decoder(decoder)
        .map_err(codec_error)?
        .into_rgba8();
    if rgba.dimensions() != (width, height) {
        return Err(ImageDecodeError::InvalidData(
            "decoded extent changed".into(),
        ));
    }
    RgbaImage::new(width, height, rgba.into_raw()).map_err(ImageDecodeError::InvalidData)
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    // Original 1x2 uncompressed 24-bit bitmap with per-row byte padding.
    fn bitmap(top_down: bool) -> Vec<u8> {
        let mut data = vec![0u8; 62];
        data[..2].copy_from_slice(b"BM");
        data[2..6].copy_from_slice(&62u32.to_le_bytes());
        data[10..14].copy_from_slice(&54u32.to_le_bytes());
        data[14..18].copy_from_slice(&40u32.to_le_bytes());
        data[18..22].copy_from_slice(&1i32.to_le_bytes());
        data[22..26].copy_from_slice(&(if top_down { -2i32 } else { 2i32 }).to_le_bytes());
        data[26..28].copy_from_slice(&1u16.to_le_bytes());
        data[28..30].copy_from_slice(&24u16.to_le_bytes());
        data[34..38].copy_from_slice(&8u32.to_le_bytes());
        let red = [0, 0, 255, 0];
        let blue = [255, 0, 0, 0];
        data[54..58].copy_from_slice(if top_down { &red } else { &blue });
        data[58..62].copy_from_slice(if top_down { &blue } else { &red });
        data
    }
    #[test]
    fn bmp_padding_and_both_row_orientations_decode_real_rgb() {
        for top_down in [false, true] {
            let decoded = decode(&bitmap(top_down), ImageDecodeLimits::default()).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (1, 2));
            assert_eq!(decoded.pixels(), &[255, 0, 0, 255, 0, 0, 255, 255]);
        }
    }
    #[test]
    fn png_preserves_straight_alpha_and_jpeg_decodes_original_dimensions() {
        let pixels = [250, 40, 20, 0, 10, 200, 30, 127];
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&pixels, 2, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let decoded = decode(&png, ImageDecodeLimits::default()).unwrap();
        assert_eq!(decoded.pixels(), &pixels);
        assert_eq!((decoded.width(), decoded.height()), (2, 1));
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(&[20, 40, 60, 80, 100, 120], 2, 1, ExtendedColorType::Rgb8)
            .unwrap();
        let decoded = decode(&jpeg, ImageDecodeLimits::default()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 1));
        assert_eq!(decoded.byte_len(), 8);
        assert!(
            decoded
                .pixels()
                .chunks_exact(4)
                .all(|pixel| pixel[3] == 255)
        );
    }
    #[test]
    fn exact_limits_and_each_preparation_budget_are_distinct_from_corruption() {
        let bytes = bitmap(false);
        let exact = ImageDecodeLimits {
            max_encoded_bytes: bytes.len(),
            max_width: 1,
            max_height: 2,
            max_decoded_bytes: 8,
        };
        assert!(decode(&bytes, exact).is_ok());
        for limits in [
            ImageDecodeLimits {
                max_encoded_bytes: bytes.len() - 1,
                ..exact
            },
            ImageDecodeLimits {
                max_height: 1,
                ..exact
            },
            ImageDecodeLimits {
                max_decoded_bytes: 7,
                ..exact
            },
            ImageDecodeLimits {
                max_encoded_bytes: 0,
                ..exact
            },
            ImageDecodeLimits {
                max_width: 16_385,
                ..exact
            },
            ImageDecodeLimits {
                max_decoded_bytes: MAX_TEXTURE_BYTES + 1,
                ..exact
            },
        ] {
            assert!(matches!(
                decode(&bytes, limits),
                Err(ImageDecodeError::Limit(_))
            ));
        }
        assert!(matches!(
            decode(&bytes[..58], ImageDecodeLimits::default()),
            Err(ImageDecodeError::InvalidData(_))
        ));
        let mut bad = bytes;
        bad[26..28].copy_from_slice(&2u16.to_le_bytes());
        assert!(decode(&bad, ImageDecodeLimits::default()).is_err());
        assert!(matches!(
            decode(b"GIF89a", ImageDecodeLimits::default()),
            Err(ImageDecodeError::Unsupported)
        ));
        assert!(matches!(
            decode(b"\x89PNG\r\n\x1a\n", ImageDecodeLimits::default()),
            Err(ImageDecodeError::InvalidData(_))
        ));
    }
    #[test]
    fn sixteen_bit_native_output_obeys_budget_before_rgba_conversion() {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[0, 0, 255, 255, 0, 0, 255, 255],
                1,
                1,
                ExtendedColorType::Rgba16,
            )
            .unwrap();
        assert!(decode(&png, ImageDecodeLimits::default()).is_ok());
        assert!(matches!(
            decode(
                &png,
                ImageDecodeLimits {
                    max_decoded_bytes: 4,
                    ..ImageDecodeLimits::default()
                }
            ),
            Err(ImageDecodeError::Limit(_))
        ));
    }
}
