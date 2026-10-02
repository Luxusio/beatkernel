//! Preparation-time half-open BGA cropping onto a transparent 256×256 canvas.
use crate::texture::RgbaImage;
use beatkernel_bms::BgaCrop;
use std::sync::Arc;

/// Copies straight RGBA pixels onto a newly allocated transparent canvas.
///
/// Negative source origins are clamped before placement. Negative destinations
/// then discard the corresponding portion of that already clipped fragment.
/// This explicit policy does not implement inclusive endpoints or overspill.
pub fn crop_canvas(image: &Arc<RgbaImage>, crop: BgaCrop) -> Result<Arc<RgbaImage>, String> {
    crop.validate()?;
    let [x1, y1, x2, y2] = crop.source_rect.map(i64::from);
    let [dx, dy] = crop.destination.map(i64::from);
    let left = x1.max(0);
    let top = y1.max(0);
    let right = x2.min(i64::from(image.width()));
    let bottom = y2.min(i64::from(image.height()));
    let fragment_width = (right - left).max(0);
    let fragment_height = (bottom - top).max(0);
    let destination_left = dx.max(0);
    let destination_top = dy.max(0);
    let destination_right = dx
        .checked_add(fragment_width)
        .ok_or("BGA placement overflow")?
        .min(256);
    let destination_bottom = dy
        .checked_add(fragment_height)
        .ok_or("BGA placement overflow")?
        .min(256);
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(256 * 256 * 4)
        .map_err(|_| "BGA canvas allocation failed")?;
    pixels.resize(256 * 256 * 4, 0);
    if destination_left < destination_right && destination_top < destination_bottom {
        let source_left = left + destination_left - dx;
        let source_top = top + destination_top - dy;
        let width = usize::try_from(destination_right - destination_left)
            .map_err(|_| "BGA width overflow")?;
        for row in 0..destination_bottom - destination_top {
            let source_offset =
                usize::try_from(((source_top + row) * i64::from(image.width()) + source_left) * 4)
                    .map_err(|_| "BGA source offset overflow")?;
            let destination_offset =
                usize::try_from(((destination_top + row) * 256 + destination_left) * 4)
                    .map_err(|_| "BGA destination offset overflow")?;
            let source = image
                .pixels()
                .get(source_offset..source_offset + width * 4)
                .ok_or("BGA source extent mismatch")?;
            pixels[destination_offset..destination_offset + width * 4].copy_from_slice(source);
        }
    }
    Ok(Arc::new(RgbaImage::new(256, 256, pixels)?))
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel_bms::ImageId;
    fn source() -> Arc<RgbaImage> {
        Arc::new(
            RgbaImage::new(
                2,
                2,
                vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            )
            .unwrap(),
        )
    }
    fn crop(rect: [i32; 4], destination: [i32; 2]) -> BgaCrop {
        BgaCrop {
            source: ImageId(0),
            source_rect: rect,
            destination,
        }
    }
    fn pixel(image: &RgbaImage, x: usize, y: usize) -> &[u8] {
        &image.pixels()[(y * 256 + x) * 4..(y * 256 + x + 1) * 4]
    }
    #[test]
    fn half_open_alpha_and_source_immutability() {
        let original = source();
        let canvas = crop_canvas(&original, crop([0, 0, 1, 2], [3, 4])).unwrap();
        assert_eq!(pixel(&canvas, 3, 4), &[1, 2, 3, 4]);
        assert_eq!(pixel(&canvas, 3, 5), &[9, 10, 11, 12]);
        assert_eq!(pixel(&canvas, 4, 4), &[0; 4]);
        assert_eq!(original.pixels(), source().pixels());
        assert_eq!(canvas.byte_len(), 262144);
    }
    #[test]
    fn negative_source_clamps_before_destination_clipping() {
        let original = source();
        let placed = crop_canvas(&original, crop([-20, -30, 2, 2], [1, 1])).unwrap();
        assert_eq!(pixel(&placed, 1, 1), &[1, 2, 3, 4]);
        let clipped = crop_canvas(&original, crop([-20, -30, 99, 99], [-1, -1])).unwrap();
        assert_eq!(pixel(&clipped, 0, 0), &[13, 14, 15, 16]);
        assert_eq!(pixel(&clipped, 1, 0), &[0; 4]);
    }
    #[test]
    fn canvas_edges_extremes_and_invalid_rectangles() {
        let original = source();
        let edge = crop_canvas(&original, crop([0, 0, 2, 2], [255, 255])).unwrap();
        assert_eq!(pixel(&edge, 255, 255), &[1, 2, 3, 4]);
        for destination in [[i32::MIN, i32::MIN], [i32::MAX, i32::MAX]] {
            let blank = crop_canvas(
                &original,
                crop([i32::MIN, i32::MIN, i32::MAX, i32::MAX], destination),
            )
            .unwrap();
            assert!(blank.pixels().iter().all(|byte| *byte == 0));
        }
        assert!(crop_canvas(&original, crop([0, 0, 0, 1], [0, 0])).is_err());
        assert!(crop_canvas(&original, crop([2, 0, 1, 1], [0, 0])).is_err());
        let outside = crop_canvas(&original, crop([4, 4, 5, 5], [0, 0])).unwrap();
        assert!(outside.pixels().iter().all(|byte| *byte == 0));
    }
    #[test]
    fn full_canvas_always_has_distinct_owned_storage() {
        let original = Arc::new(RgbaImage::new(256, 256, vec![7; 262144]).unwrap());
        let canvas = crop_canvas(&original, crop([0, 0, 256, 256], [0, 0])).unwrap();
        assert!(!Arc::ptr_eq(&original, &canvas));
        assert_eq!(original.pixels(), canvas.pixels());
    }
}
