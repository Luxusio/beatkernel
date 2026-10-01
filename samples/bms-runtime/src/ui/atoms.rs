//! Drawing atoms: clipped rectangles and original bitmap glyph text.
use crate::scene::Scene;

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

#[cfg(test)]
mod tests {
    use super::*;
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
