//! Immutable prepared-font text geometry; no rasterization or native ownership.
use crate::{
    font_atlas::{FontAtlas, Glyph},
    scene::{ClipRect, Scene},
    texture::TextureId,
    ui::text_input::LineEditor,
};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

/// Maximum scalar prefix considered for one single-line draw.
pub const MAX_TEXT_GLYPHS: usize = 1024;

/// Borrowed field window with decorations in pixels from the text origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontFieldLine<'a> {
    pub value: &'a str,
    pub caret_x: i64,
    pub caret_visible: bool,
    pub composition: Option<(i64, i64)>,
    pub selection: Option<(i64, i64)>,
}

/// A fixed-scale prepared atlas paired with its renderer-owned texture identity.
/// This draws independent glyphs, without shaping, kerning or fallback fonts.
#[derive(Clone)]
pub struct FontText {
    atlas: Arc<FontAtlas>,
    texture: TextureId,
    ascent: i64,
}
impl PartialEq for FontText {
    fn eq(&self, other: &Self) -> bool {
        self.texture == other.texture && Arc::ptr_eq(&self.atlas, &other.atlas)
    }
}
impl Eq for FontText {}
impl FontText {
    pub fn new(atlas: Arc<FontAtlas>, texture: TextureId) -> Result<Self, String> {
        let ascent = pixel(f64::from(atlas.ascent()).ceil())?;
        Ok(Self {
            atlas,
            texture,
            ascent,
        })
    }
    pub const fn texture_id(&self) -> TextureId {
        self.texture
    }
    /// Validates the complete bounded editor, then borrows a grapheme-aligned
    /// window. This only reads cached metrics: it neither allocates nor prepares
    /// glyphs on success. Decorations use the same rebased pen as drawing.
    pub fn field_line<'a>(
        &self,
        editor: &'a LineEditor,
        width: u32,
    ) -> Result<FontFieldLine<'a>, String> {
        let value = editor.value();
        self.measure(value)?;
        let cursor = editor.cursor();
        let composition = editor.composition();
        let caret_visible = composition.is_none_or(|range| range.selection.is_some());
        if width == 0 {
            return Ok(FontFieldLine {
                value: &value[cursor..cursor],
                caret_x: 0,
                caret_visible,
                composition: None,
                selection: None,
            });
        }
        let available = f64::from(width);
        let caret_cluster = value.grapheme_indices(true).find(|&(at, cluster)| {
            (cursor == 0 && at == 0) || (at < cursor && cursor <= at + cluster.len())
        });
        if caret_cluster.is_some_and(|(_, cluster)| cluster.chars().count() > MAX_TEXT_GLYPHS) {
            return Err("caret grapheme exceeds the font scalar budget".into());
        }
        // Prefer the full composition when both pixel and scalar budgets admit
        // its enclosing clusters. Native scalar endpoints themselves stay intact.
        let fitting_composition = if let Some(composition) = composition {
            let begin = value
                .grapheme_indices(true)
                .take_while(|&(at, _)| at <= composition.range.0)
                .last()
                .map_or(0, |(at, _)| at);
            let end = value
                .grapheme_indices(true)
                .find(|&(at, _)| at >= composition.range.1)
                .map_or(value.len(), |(at, _)| at);
            let (advance, count) = self.measure(&value[begin..end])?;
            (advance <= available && count <= MAX_TEXT_GLYPHS).then_some((begin, end))
        } else {
            None
        };
        let required = fitting_composition.unwrap_or_else(|| {
            caret_cluster
                .filter(|&(at, cluster)| at < cursor && cursor < at + cluster.len())
                .map_or((cursor, cursor), |(at, cluster)| (at, at + cluster.len()))
        });
        let target = required.1;
        let mut start = target;
        let mut advance = 0.0;
        let mut count = 0;
        for (at, cluster) in value
            .grapheme_indices(true)
            .rev()
            .filter(|&(at, _)| at < target)
        {
            let (cluster_advance, scalars) = self.measure(cluster)?;
            let next = advance + cluster_advance;
            if scalars > MAX_TEXT_GLYPHS - count || (next > available && at < required.0) {
                break;
            }
            start = at;
            advance = next;
            count += scalars;
        }
        // Reverse accumulation chooses the window; forward accumulation is
        // authoritative for painting. A rounding discrepancy cannot move the
        // caret outside the field or split a composition that fits by itself.
        if self.measure(&value[start..target])?.0 > available {
            start = required.0;
        }
        let mut end = start;
        let mut pen = 0.0;
        let mut count = 0;
        for (at, cluster) in value.grapheme_indices(true).filter(|&(at, _)| at >= start) {
            let scalars = cluster.chars().count();
            if scalars > MAX_TEXT_GLYPHS - count {
                break;
            }
            // Keep zero-advance clusters through the target even at the exact
            // right boundary. The borrowed window must still contain its caret.
            if pen >= available && at >= target {
                break;
            }
            for character in cluster.chars() {
                pen += f64::from(self.cached_glyph(character)?.advance);
            }
            count += scalars;
            end = at + cluster.len();
        }
        let position = |byte: usize| -> Result<i64, String> {
            let byte = byte.clamp(start, end);
            pixel(self.measure(&value[start..byte])?.0.round())
        };
        let clip = |range: (usize, usize)| -> Result<Option<(i64, i64)>, String> {
            let begin = position(range.0)?.clamp(0, i64::from(width));
            let end = position(range.1)?.clamp(0, i64::from(width));
            Ok((begin < end).then_some((begin, end)))
        };
        Ok(FontFieldLine {
            value: &value[start..end],
            caret_x: position(cursor)?.clamp(0, i64::from(width)),
            caret_visible,
            composition: composition
                .map(|range| clip(range.range))
                .transpose()?
                .flatten(),
            selection: composition
                .map_or_else(|| editor.selection(), |range| range.selection)
                .map(clip)
                .transpose()?
                .flatten(),
        })
    }
    fn cached_glyph(&self, character: char) -> Result<Glyph, String> {
        let glyph = self
            .atlas
            .get(character)
            .ok_or_else(|| format!("font text character {character:?} was not prepared"))?;
        if !glyph.advance.is_finite() || glyph.advance < 0.0 {
            return Err("font text advance must be finite and nonnegative".into());
        }
        Ok(glyph)
    }
    fn measure(&self, value: &str) -> Result<(f64, usize), String> {
        let mut pen = 0.0;
        let mut count = 0;
        for character in value.chars() {
            pen += f64::from(self.cached_glyph(character)?.advance);
            pixel(pen.ceil())?;
            count += 1;
        }
        Ok((pen, count))
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
            let glyph = self.cached_glyph(character)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_fixture::font_bytes;

    fn field_font(space_units: u16, prepared: &str) -> FontText {
        let mut bytes = font_bytes();
        let table = |tag: &[u8; 4]| {
            (0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize)
                .map(|index| 12 + index * 16)
                .find(|&row| &bytes[row..row + 4] == tag)
                .unwrap()
        };
        let hmtx_row = table(b"hmtx");
        let head_row = table(b"head");
        let offset =
            u32::from_be_bytes(bytes[hmtx_row + 8..hmtx_row + 12].try_into().unwrap()) as usize;
        let head =
            u32::from_be_bytes(bytes[head_row + 8..head_row + 12].try_into().unwrap()) as usize;
        // At scale 10 and 1000-unit height: tofu/é advances 4.5 px,
        // A/가 advances 8 px, and ordinary space advances 2 px.
        for (index, units) in [450u16, 800, space_units].into_iter().enumerate() {
            bytes[offset + index * 4..offset + index * 4 + 2].copy_from_slice(&units.to_be_bytes());
        }
        fn checksum(bytes: &[u8]) -> u32 {
            bytes.chunks(4).fold(0u32, |sum, part| {
                let mut word = [0; 4];
                word[..part.len()].copy_from_slice(part);
                sum.wrapping_add(u32::from_be_bytes(word))
            })
        }
        let hmtx_checksum = checksum(&bytes[offset..offset + 12]);
        bytes[hmtx_row + 4..hmtx_row + 8].copy_from_slice(&hmtx_checksum.to_be_bytes());
        bytes[head + 8..head + 12].fill(0);
        let adjustment = 0xb1b0_afbau32.wrapping_sub(checksum(&bytes));
        bytes[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
        let mut atlas = FontAtlas::new(bytes, 10.0, 128, 128, 16).unwrap();
        for character in prepared.chars() {
            atlas.prepare(character).unwrap();
        }
        FontText::new(Arc::new(atlas), TextureId::allocate().unwrap()).unwrap()
    }

    #[test]
    fn binding_equality_tracks_atlas_generation_and_texture_identity() {
        let font = field_font(200, "A ");
        assert!(font == font.clone());
        assert_eq!(font.texture_id(), font.texture);
        let rebound =
            FontText::new(Arc::clone(&font.atlas), TextureId::allocate().unwrap()).unwrap();
        assert!(font != rebound);
        let same_atlas = FontAtlas::extend_texts(&font.atlas, &[" A"]).unwrap();
        assert!(font == FontText::new(same_atlas, font.texture_id()).unwrap());
        let extended = FontAtlas::extend_texts(&font.atlas, &["가"]).unwrap();
        assert!(font != FontText::new(extended, font.texture_id()).unwrap());
    }

    #[test]
    fn proportional_unicode_selection_and_clipping_use_rebased_positions() {
        let font = field_font(200, "A가 é");
        let mut editor = LineEditor::new("A가 éA", 32).unwrap();
        let line = font.field_line(&editor, 100).unwrap();
        assert_eq!(line.value, "A가 éA");
        assert_eq!(line.caret_x, 31);
        assert!(line.caret_visible);
        assert_eq!(line.selection, None);
        editor.home();
        editor.right();
        editor.move_right(true);
        editor.move_right(true);
        let line = font.field_line(&editor, 100).unwrap();
        assert_eq!((line.caret_x, line.selection), (18, Some((8, 18))));
        editor.clear_selection();
        editor.end();
        editor.move_home(true);
        let reversed = font.field_line(&editor, 100).unwrap();
        assert_eq!((reversed.caret_x, reversed.selection), (0, Some((0, 31))));
        editor.select_all();
        let clipped = font.field_line(&editor, 17).unwrap();
        assert_eq!(clipped.value, " éA");
        assert_eq!((clipped.caret_x, clipped.selection), (15, Some((0, 15))));

        // Rounding absolute glyph positions and subtracting them would place
        // this A at 4 px. Drawing the borrowed line starts a fresh pen at zero.
        let editor = LineEditor::new("ééA", 32).unwrap();
        let line = font.field_line(&editor, 13).unwrap();
        assert_eq!(line.value, "éA");
        assert_eq!(line.caret_x, 13);
        let mut scene = Scene::new(100, 32);
        font.draw(&mut scene, 0, 0, line.value, 0xffffff).unwrap();
        assert_eq!(scene.rectangles().len(), 2);
        assert_eq!(scene.rectangles()[0].bounds[0], 0.0);
        assert_eq!(scene.rectangles()[1].bounds[0], 5.0);
    }

    #[test]
    fn composition_prefers_complete_text_and_preserves_native_cursor_visibility() {
        let font = field_font(200, "A가 é");
        let mut base = LineEditor::new("A A", 32).unwrap();
        base.left();
        let preview = base.preedit("가é", Some((0, 5))).unwrap();
        let complete = font.field_line(&preview, 15).unwrap();
        assert_eq!(complete.value, " 가éA");
        assert_eq!(complete.caret_x, 2);
        assert!(complete.caret_visible);
        assert_eq!(complete.composition, Some((2, 15)));
        assert_eq!(complete.selection, Some((2, 15)));
        let clipped = font.field_line(&preview, 9).unwrap();
        assert_eq!(clipped.value, " 가");
        assert_eq!(clipped.caret_x, 2);
        assert_eq!(clipped.composition, Some((2, 9)));
        assert_eq!(clipped.selection, Some((2, 9)));
        let collapsed = base.preedit("가é", Some((3, 3))).unwrap();
        let line = font.field_line(&collapsed, 15).unwrap();
        assert_eq!(line.caret_x, 10);
        assert_eq!(line.selection, None);
        assert!(line.caret_visible);
        let hidden = base.preedit("가é", None).unwrap();
        let line = font.field_line(&hidden, 15).unwrap();
        assert_eq!((line.caret_x, line.caret_visible), (15, false));
        assert_eq!(line.composition, Some((2, 15)));
        assert_eq!(line.selection, None);
        let empty = font.field_line(&hidden, 0).unwrap();
        assert_eq!(empty.value, "");
        assert_eq!((empty.caret_x, empty.caret_visible), (0, false));
        assert_eq!((empty.composition, empty.selection), (None, None));
        let cancelled = base.preedit("", None).unwrap();
        let line = font.field_line(&cancelled, 100).unwrap();
        assert_eq!(line.value, "A A");
        assert_eq!((line.caret_x, line.caret_visible), (10, true));
        assert_eq!((line.composition, line.selection), (None, None));
        assert_eq!(base.value(), "A A");
        assert_eq!(base.cursor(), 2);
    }

    #[test]
    fn zero_and_subglyph_widths_keep_the_caret_and_clip_positive_ranges() {
        let font = field_font(200, "A");
        let mut editor = LineEditor::new("AA", 16).unwrap();
        let end = font.field_line(&editor, 1).unwrap();
        assert_eq!((end.value, end.caret_x), ("", 0));
        editor.move_home(true);
        let start = font.field_line(&editor, 1).unwrap();
        assert_eq!((start.value, start.caret_x), ("A", 0));
        assert_eq!(start.selection, Some((0, 1)));
        let empty = font.field_line(&editor, 0).unwrap();
        assert_eq!((empty.value, empty.caret_x), ("", 0));
        assert!(empty.caret_visible);
        assert_eq!(empty.selection, None);
    }

    #[test]
    fn zero_and_tiny_advances_keep_long_text_carets_inside_the_scalar_budget() {
        for (space_units, end_x) in [(0, 0), (1, 10)] {
            let font = field_font(space_units, " ");
            let value = " ".repeat(4096);
            let mut editor = LineEditor::new(&value, 4096).unwrap();
            let line = font.field_line(&editor, 100).unwrap();
            assert_eq!(line.value.len(), 1024);
            assert_eq!(line.value.as_ptr(), editor.value()[3072..].as_ptr());
            assert_eq!(line.caret_x, end_x);
            editor.home();
            for _ in 0..2048 {
                editor.right();
            }
            let middle = font.field_line(&editor, 100).unwrap();
            assert_eq!(middle.value.len(), 1024);
            assert_eq!(middle.value.as_ptr(), editor.value()[1024..].as_ptr());
            assert_eq!(middle.caret_x, end_x);
        }
        let font = field_font(0, "A ");
        let value = format!("A{}", " ".repeat(2048));
        let base = LineEditor::new("", 4096).unwrap();
        let preview = base
            .preedit(&value, Some((value.len(), value.len())))
            .unwrap();
        let line = font.field_line(&preview, 8).unwrap();
        assert_eq!(line.value.len(), 1024);
        assert_eq!(line.caret_x, 0);
        assert!(line.caret_visible);
        assert_eq!(line.composition, None); // The admitted spaces have zero width.

        let preview = base.preedit("A   ", Some((4, 4))).unwrap();
        let exact = font.field_line(&preview, 8).unwrap();
        assert_eq!(exact.value, "A   ");
        assert_eq!(exact.caret_x, 8);
        assert_eq!(exact.composition, Some((0, 8)));
    }

    #[test]
    fn uncached_text_is_rejected_even_outside_the_visible_or_zero_width_window() {
        let font = field_font(200, "A");
        let mut editor = LineEditor::new("A가", 16).unwrap();
        editor.home();
        for width in [0, 1, 100] {
            assert_eq!(
                font.field_line(&editor, width).unwrap_err(),
                "font text character '가' was not prepared"
            );
        }
        let value = format!("{}가", "A".repeat(2048));
        let mut editor = LineEditor::new(&value, 4096).unwrap();
        editor.home();
        assert!(font.field_line(&editor, 8).is_err());
        assert_eq!(font.atlas.len(), 1);
    }
}
