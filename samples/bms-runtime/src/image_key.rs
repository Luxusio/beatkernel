//! Immutable exact-black transparency preparation for declared BGA Layer assets.
use crate::texture::RgbaImage;
use std::sync::Arc;

pub(crate) fn needs_key(image: &RgbaImage) -> bool {
    image
        .pixels()
        .chunks_exact(4)
        .any(|pixel| pixel[..3] == [0, 0, 0] && pixel[3] != 0)
}

/// Clears alpha only for exact RGB black. Nonblack RGBA and the original pixels
/// remain untouched; unchanged/already-transparent input shares the same Arc.
/// Called once during preparation, never from rendering or native callbacks.
pub fn black_to_transparent(image: &Arc<RgbaImage>) -> Result<Arc<RgbaImage>, String> {
    if !needs_key(image) {
        return Ok(Arc::clone(image));
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(image.pixels().len())
        .map_err(|e| e.to_string())?;
    pixels.extend_from_slice(image.pixels());
    for pixel in pixels.chunks_exact_mut(4) {
        if pixel[..3] == [0, 0, 0] {
            pixel[3] = 0;
        }
    }
    Ok(Arc::new(RgbaImage::new(
        image.width(),
        image.height(),
        pixels,
    )?))
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn exact_black_partial_alpha_and_nearblack_preserve_originals() {
        let pixels = vec![
            0, 0, 0, 255, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 1, 255, 1, 0, 0, 37, 42, 7, 13, 128,
        ];
        let original = Arc::new(RgbaImage::new(6, 1, pixels.clone()).unwrap());
        let keyed = black_to_transparent(&original).unwrap();
        assert!(!Arc::ptr_eq(&keyed, &original));
        assert_eq!(original.pixels(), pixels);
        assert_eq!(
            keyed.pixels(),
            &[
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 255, 1, 0, 0, 37, 42, 7, 13, 128
            ]
        );
        assert!(Arc::ptr_eq(&black_to_transparent(&keyed).unwrap(), &keyed));
        let alias = original.clone();
        assert!(Arc::ptr_eq(&alias, &original));
        assert_eq!(alias.pixels(), pixels);
    }
    #[test]
    fn unchanged_and_transparent_black_share_without_allocation() {
        for pixels in [vec![0, 0, 0, 0], vec![1, 0, 0, 255], vec![0, 0, 1, 0]] {
            let original = Arc::new(RgbaImage::new(1, 1, pixels).unwrap());
            assert!(Arc::ptr_eq(
                &black_to_transparent(&original).unwrap(),
                &original
            ));
        }
    }
}
