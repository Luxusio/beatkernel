//! Immutable prepared-font text geometry; no rasterization or native ownership.
use crate::{
    font_atlas::{FontAtlas, Glyph},
    scene::{ClipRect, Scene},
    texture::TextureId,
};
use std::sync::Arc;

/// Maximum scalar prefix considered for one single-line draw.
pub const MAX_TEXT_GLYPHS: usize = 1024;

/// A fixed-scale prepared atlas paired with its renderer-owned texture identity.
/// This draws independent glyphs, without shaping, kerning or fallback fonts.
#[derive(Clone)]
pub struct FontText {
    atlas: Arc<FontAtlas>,
    texture: TextureId,
    ascent: i64,
}
impl FontText {
    pub fn new(atlas: Arc<FontAtlas>, texture: TextureId) -> Result<Self, String> {
        let ascent = pixel(f64::from(atlas.ascent()).ceil())?;
        Ok(Self {
            atlas,
            texture,
            ascent,
        })
    }
    /// Draws the prepared visible prefix with original character case and actual
    /// font advances. Cache/metric/position errors are checked before geometry
    /// writes; Scene retains its own clipping and geometry-budget error behavior.
    /// A negative origin is rejected; an origin outside the viewport is a no-op.
    pub fn draw(
        &self,
        scene: &mut Scene,
        x: i64,
        y: i64,
        value: &str,
        color: u32,
    ) -> Result<(), String> {
        self.draw_in(scene, x, y, value, color, None)
    }
    /// Draws into an explicit component clip, intersected with the viewport.
    /// Glyphs retain their original metrics; clipping crops texture UVs.
    /// The same immutable clip applies to both preflight and geometry passes.
    pub fn draw_clipped(
        &self,
        scene: &mut Scene,
        x: i64,
        y: i64,
        value: &str,
        color: u32,
        clip: ClipRect,
    ) -> Result<(), String> {
        self.draw_in(scene, x, y, value, color, Some(clip))
    }
    fn draw_in(
        &self,
        scene: &mut Scene,
        x: i64,
        y: i64,
        value: &str,
        color: u32,
        clip: Option<ClipRect>,
    ) -> Result<(), String> {
        if x < 0 || y < 0 {
            return Err("font text origin must be nonnegative".into());
        }
        let [width, height] = scene.dimensions();
        let (right, bottom) = if let Some(clip) = clip {
            let Some([_, _, right, bottom]) = scene.clip_bounds(clip) else {
                return Ok(());
            };
            (right as f64, bottom as f64)
        } else {
            (f64::from(width), f64::from(height))
        };
        if x as f64 >= right || y as f64 >= bottom {
            return Ok(());
        }
        scene.status()?;
        let baseline = y
            .checked_add(self.ascent)
            .ok_or("font text baseline overflow")?;
        self.walk(x, baseline, right, value, |_, _| Ok(()))?;
        self.walk(x, baseline, right, value, |glyph, bounds| {
            if let Some(uv) = glyph.uv {
                if let Some(clip) = clip {
                    scene.sprite_clipped(self.texture, bounds, uv, color, clip)?;
                } else {
                    scene.sprite(self.texture, bounds, uv, color)?;
                }
            }
            Ok(())
        })
    }
    /// Both passes use exactly the same immutable cached metrics and stopping
    /// condition. No glyph vector or draw-time atlas preparation is needed.
    fn walk(
        &self,
        x: i64,
        baseline: i64,
        width: f64,
        value: &str,
        mut visit: impl FnMut(Glyph, [i64; 4]) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut pen = x as f64;
        for character in value.chars().take(MAX_TEXT_GLYPHS) {
            if pen >= width {
                break;
            }
            let glyph = self
                .atlas
                .get(character)
                .ok_or_else(|| format!("font text character {character:?} was not prepared"))?;
            if !glyph.advance.is_finite() || glyph.advance < 0.0 {
                return Err("font text advance must be finite and nonnegative".into());
            }
            let left = pixel(pen.round())?
                .checked_add(i64::from(glyph.bounds[0]))
                .ok_or("font text horizontal placement overflow")?;
            let top = baseline
                .checked_add(i64::from(glyph.bounds[1]))
                .ok_or("font text vertical placement overflow")?;
            let glyph_width = i64::from(glyph.bounds[2]);
            let glyph_height = i64::from(glyph.bounds[3]);
            left.checked_add(glyph_width)
                .ok_or("font text horizontal extent overflow")?;
            top.checked_add(glyph_height)
                .ok_or("font text vertical extent overflow")?;
            pen += f64::from(glyph.advance);
            pixel(pen.ceil())?;
            visit(glyph, [left, top, glyph_width, glyph_height])?;
        }
        Ok(())
    }
}

fn pixel(value: f64) -> Result<i64, String> {
    // The upper endpoint is exclusive: i64::MAX rounds to 2^63 as f64.
    if !value.is_finite() || value < i64::MIN as f64 || value >= -(i64::MIN as f64) {
        return Err("font text pixel coordinate is not representable".into());
    }
    Ok(value as i64)
}
