//! Drawing atoms: clipped rectangles and original bitmap glyph text.
use crate::{
    font_text::MAX_TEXT_GLYPHS,
    scene::{ClipRect, Scene},
    texture::TextureId,
};

pub fn rect(pixels: &mut Scene, x: i64, y: i64, width: i64, height: i64, color: u32) {
    pixels.rect(x, y, width, height, color);
}

pub fn text(pixels: &mut Scene, x: usize, y: usize, value: &str, scale: usize, color: u32) {
    let [width, height] = pixels.dimensions();
    if scale == 0 || y >= height as usize {
        return;
    }
    let stride = 6usize.saturating_mul(scale);
    let limit = (width as usize).saturating_sub(x) / stride;
    for (index, character) in value.chars().take(limit).enumerate() {
        pixels.glyph(
            [
                (x + index * 6 * scale) as i64,
                y as i64,
                5 * scale as i64,
                7 * scale as i64,
            ],
            crate::font::glyph_uv(character),
            color,
        );
    }
}

/// Bitmap text within an explicit component clip, including partial glyphs.
/// Invalid scale arithmetic rejects before geometry; no clip state is retained.
pub fn text_clipped(
    pixels: &mut Scene,
    x: usize,
    y: usize,
    value: &str,
    scale: usize,
    color: u32,
    clip: ClipRect,
) -> Result<(), String> {
    let Some([_, _, right, bottom]) = pixels.clip_bounds(clip) else {
        return Ok(());
    };
    if scale == 0 || x >= right as usize || y >= bottom as usize {
        return Ok(());
    }
    pixels.status()?;
    let stride = scale.checked_mul(6).ok_or("bitmap text stride overflow")?;
    let width = scale
        .checked_mul(5)
        .and_then(|n| i64::try_from(n).ok())
        .ok_or("bitmap text glyph width overflow")?;
    let height = scale
        .checked_mul(7)
        .and_then(|n| i64::try_from(n).ok())
        .ok_or("bitmap text glyph height overflow")?;
    let limit = (right as usize - x).div_ceil(stride).min(MAX_TEXT_GLYPHS);
    for (index, character) in value.chars().take(limit).enumerate() {
        // The last pen is strictly below the viewport-intersected right edge.
        pixels.sprite_clipped(
            TextureId::FONT,
            [(x + index * stride) as i64, y as i64, width, height],
            crate::font::glyph_uv(character),
            color,
            clip,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bitmap_component_clip_keeps_partial_glyph_uvs_and_does_not_clip_siblings() {
        let mut scene = Scene::new(64, 64);
        let clip = ClipRect::new([6, 3, 8, 4]).unwrap();
        text_clipped(&mut scene, 4, 3, "ABC", 1, 0xffffff, clip).unwrap();
        assert_eq!(scene.rectangles().len(), 2);
        assert_eq!(scene.rectangles()[0].bounds, [6.0, 3.0, 3.0, 4.0]);
        assert_eq!(scene.rectangles()[1].bounds, [10.0, 3.0, 4.0, 4.0]);
        let original = crate::font::glyph_uv('B');
        assert_eq!(scene.rectangles()[1].uv[0], original[0]);
        assert!((scene.rectangles()[1].uv[2] - original[2] * 4.0 / 5.0).abs() < 0.000001);
        assert!((scene.rectangles()[1].uv[3] - original[3] * 4.0 / 7.0).abs() < 0.000001);
        let epoch = scene.geometry_stamp().1;
        assert!(text_clipped(&mut scene, 4, 3, "A", usize::MAX, 0xffffff, clip).is_err());
        text_clipped(&mut scene, 4, 3, "A", 0, 0xffffff, clip).unwrap();
        text_clipped(&mut scene, usize::MAX, usize::MAX, "A", 1, 0xffffff, clip).unwrap();
        assert_eq!(scene.geometry_stamp().1, epoch);
        text(&mut scene, 28, 3, "A", 1, 0xffffff);
        assert_eq!(scene.rectangles()[2].bounds, [28.0, 3.0, 5.0, 7.0]);
    }
    #[test]
    fn text_handles_invalid_scale_and_outside_origin() {
        let mut scene = Scene::new(960, 720);
        text(&mut scene, 0, 0, "AB", 0, 0xffffff);
        text(
            &mut scene,
            usize::MAX,
            usize::MAX,
            "AB",
            usize::MAX,
            0xffffff,
        );
        assert!(scene.rectangles().is_empty());
        text(&mut scene, 0, 0, "AB", 1, 0xffffff);
        assert!(!scene.rectangles().is_empty());
        assert_ne!(crate::font::glyph('A'), crate::font::glyph('B'));
    }
}
