//! Small compositions of drawing atoms, reusable across screens.
use super::atoms::{rect, text};
use super::interaction::Bounds;
use super::text_input::{LineEditor, VisibleLine};
use crate::{
    font_text::FontText,
    gauge::{BmsGauge, GaugeFailure, GAUGE_UNITS_PER_PERCENT, MAX_GAUGE_UNITS},
    scene::{ClipRect, Scene},
    texture::TextureId,
};
use std::ops::RangeInclusive;

/// Draws only the borrowed gauge snapshot; READY is not song completion.
/// Bounds are validated before geometry and the ASCII label uses stack storage.
pub fn gauge_hud(scene: &mut Scene, gauge: &BmsGauge, bounds: Bounds) -> Result<(), String> {
    if bounds.x < 0 || bounds.y < 0 || bounds.width <= 0 || bounds.height < 14 {
        return Err(
            "gauge HUD requires nonnegative origin, positive width and height at least 14".into(),
        );
    }
    let clip = ClipRect::new([bounds.x, bounds.y, bounds.width, bounds.height])?;
    let text_x = usize::try_from(bounds.x.checked_add(2).ok_or("gauge label x overflow")?)
        .map_err(|_| "gauge label x exceeds usize")?;
    let text_y = usize::try_from(bounds.y.checked_add(2).ok_or("gauge label y overflow")?)
        .map_err(|_| "gauge label y exceeds usize")?;
    let bar_y = bounds
        .y
        .checked_add(bounds.height - 4)
        .ok_or("gauge bar y overflow")?;
    scene.status()?;

    let snapshot = gauge.snapshot();
    let (prefix, color) = match snapshot.failure {
        Some(GaugeFailure::InstantDeath) => ("DEAD", 0xef6372),
        Some(GaugeFailure::Depleted) => ("EMPTY", 0xd8b36b),
        None if gauge.can_clear() => ("READY", 0x61d69a),
        None => ("GAUGE", 0x4f92db),
    };
    let hundredths = snapshot.level_units / (GAUGE_UNITS_PER_PERCENT / 100);
    let whole = hundredths / 100;
    let mut label = [0u8; 16];
    let mut length = prefix.len();
    label[..length].copy_from_slice(prefix.as_bytes());
    label[length] = b' ';
    length += 1;
    if whole >= 100 {
        label[length] = b'1';
        length += 1;
    }
    if whole >= 10 {
        label[length] = b'0' + ((whole / 10) % 10) as u8;
        length += 1;
    }
    label[length] = b'0' + (whole % 10) as u8;
    label[length + 1] = b'.';
    label[length + 2] = b'0' + ((hundredths / 10) % 10) as u8;
    label[length + 3] = b'0' + (hundredths % 10) as u8;
    label[length + 4] = b'%';
    length += 5;
    let label = std::str::from_utf8(&label[..length]).expect("gauge label is ASCII");
    let filled = (i128::from(bounds.width) * i128::from(snapshot.level_units)
        / i128::from(MAX_GAUGE_UNITS))
    .clamp(0, i128::from(bounds.width)) as i64;
    rect(
        scene,
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        0x1f2d40,
    );
    rect(scene, bounds.x, bar_y, filled, 4, color);
    super::atoms::text_clipped(scene, text_x, text_y, label, 1, color, clip)?;
    scene.status()
}

/// A clipped line field; editing and focus belong to the menu owner.
pub fn text_field(scene: &mut Scene, editor: &LineEditor, bounds: Bounds, focused: bool) {
    if bounds.width < 16 || bounds.height < 30 {
        return;
    }
    let cells = usize::try_from((bounds.width - 16) / 12).unwrap_or(usize::MAX);
    paint_line(scene, editor.visible_line(cells), bounds, focused);
}

/// Font preparation belongs to the caller; this paints immutable cached geometry.
pub fn text_field_with_font(
    scene: &mut Scene,
    editor: &LineEditor,
    bounds: Bounds,
    focused: bool,
    font: Option<&FontText>,
) {
    let Some(font) = font else {
        return text_field(scene, editor, bounds, focused);
    };
    if bounds.width < 18 || bounds.height < 30 {
        return;
    }
    let result = (|| {
        let width = u32::try_from(bounds.width - 18).map_err(|_| "font field width exceeds u32")?;
        let line = font.field_line(editor, width)?;
        let x = bounds.x.checked_add(8).ok_or("font field x overflow")?;
        let y = bounds.y.checked_add(8).ok_or("font field y overflow")?;
        let clip = ClipRect::new([x, y, bounds.width - 16, 18])?;
        rect(
            scene,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            if focused { 0x354e6c } else { 0x263d59 },
        );
        if let Some((start, end)) = line.selection.filter(|_| focused) {
            scene.sprite_clipped(
                TextureId::WHITE,
                [x + start, y, end - start, 16],
                [0.0, 0.0, 1.0, 1.0],
                0x42688a,
                clip,
            )?;
        }
        font.draw_clipped(scene, x, y, line.value, 0xf0f4ff, clip)?;
        if let Some((start, end)) = line.composition.filter(|_| focused) {
            scene.sprite_clipped(
                TextureId::WHITE,
                [x + start, y + 16, end - start, 2],
                [0.0, 0.0, 1.0, 1.0],
                0x74e5c5,
                clip,
            )?;
        }
        if focused && line.caret_visible {
            scene.sprite_clipped(
                TextureId::WHITE,
                [x + line.caret_x, y, 2, 16],
                [0.0, 0.0, 1.0, 1.0],
                0x74e5c5,
                clip,
            )?;
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        scene.reject(error);
    }
}

pub fn text_field_value_with_font(
    scene: &mut Scene,
    value: &str,
    bounds: Bounds,
    font: Option<&FontText>,
) {
    let Some(font) = font else {
        return text_field_value(scene, value, bounds);
    };
    if bounds.width < 18 || bounds.height < 30 {
        return;
    }
    let result = (|| {
        let x = bounds.x.checked_add(8).ok_or("font field x overflow")?;
        let y = bounds.y.checked_add(8).ok_or("font field y overflow")?;
        let clip = ClipRect::new([x, y, bounds.width - 16, 18])?;
        rect(
            scene,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            0x263d59,
        );
        font.draw_clipped(scene, x, y, value, 0xf0f4ff, clip)
    })();
    if let Err(error) = result {
        scene.reject(error);
    }
}

/// An unfocused field borrows its value without constructing an editor per frame.
pub fn text_field_value(scene: &mut Scene, value: &str, bounds: Bounds) {
    if bounds.width < 16 || bounds.height < 30 {
        return;
    }
    let cells = usize::try_from((bounds.width - 16) / 12).unwrap_or(usize::MAX);
    let end = value
        .char_indices()
        .nth(cells)
        .map_or(value.len(), |(at, _)| at);
    paint_line(
        scene,
        VisibleLine {
            value: &value[..end],
            caret: 0,
            caret_visible: true,
            composition: None,
            selection: None,
        },
        bounds,
        false,
    );
}

fn paint_line(scene: &mut Scene, line: VisibleLine<'_>, bounds: Bounds, focused: bool) {
    rect(
        scene,
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        if focused { 0x354e6c } else { 0x263d59 },
    );
    let x = bounds.x.saturating_add(8);
    let y = bounds.y.saturating_add(8);
    let cell_offset = |column| i64::try_from(column).unwrap_or(i64::MAX).saturating_mul(12);
    if let Some((start, end)) = line.selection.filter(|_| focused) {
        rect(
            scene,
            x.saturating_add(cell_offset(start)),
            y,
            cell_offset(end - start),
            16,
            0x42688a,
        );
    }
    text(
        scene,
        usize::try_from(x).unwrap_or(usize::MAX),
        usize::try_from(y).unwrap_or(usize::MAX),
        line.value,
        2,
        0xf0f4ff,
    );
    if let Some((start, end)) = line.composition.filter(|_| focused) {
        rect(
            scene,
            x.saturating_add(cell_offset(start)),
            y.saturating_add(16),
            cell_offset(end - start),
            2,
            0x74e5c5,
        );
    }
    if focused && line.caret_visible {
        let offset = cell_offset(line.caret);
        rect(scene, x.saturating_add(offset), y, 2, 16, 0x74e5c5);
    }
}

/// A view-only button. The menu owner handles hit testing and activation.
pub fn button(scene: &mut Scene, bounds: Bounds, label: &str, hovered: bool, pressed: bool) {
    if bounds.width <= 0 || bounds.height <= 0 {
        return;
    }
    let color = if pressed {
        0x42688a
    } else if hovered {
        0x354e6c
    } else {
        0x263d59
    };
    rect(
        scene,
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        color,
    );
    if bounds.height < 30 {
        return;
    }
    let characters = usize::try_from(bounds.width.saturating_sub(16) / 12).unwrap_or(usize::MAX);
    let end = label
        .char_indices()
        .nth(characters)
        .map_or(label.len(), |(index, _)| index);
    text(
        scene,
        usize::try_from(bounds.x.max(0))
            .unwrap_or(usize::MAX)
            .saturating_add(8),
        usize::try_from(bounds.y.max(0))
            .unwrap_or(usize::MAX)
            .saturating_add(8),
        &label[..end],
        2,
        0xf0f4ff,
    );
}

pub fn counter(scene: &mut Scene, x: usize, y: usize, label: &str, value: u64, color: u32) {
    text(scene, x, y, label, 2, color);
    text(
        scene,
        x,
        y.saturating_add(30),
        &value.to_string(),
        3,
        0xffffff,
    );
}

/// A note head and optional hold body/tail, clipped to a visible lane region.
pub fn note(
    scene: &mut Scene,
    lane: (i64, i64),
    head: i64,
    tail: Option<i64>,
    visible: RangeInclusive<i64>,
) {
    let (left, right) = lane;
    if right <= left || visible.is_empty() {
        return;
    }
    let lane_width = right.saturating_sub(left);
    let top = *visible.start();
    let bottom = *visible.end();
    if let Some(tail) = tail {
        rect(
            scene,
            left.saturating_add(6),
            tail.max(top),
            lane_width.saturating_sub(12).max(1),
            head.min(bottom).saturating_sub(tail.max(top)),
            0x357e98,
        );
        if visible.contains(&tail) {
            rect(
                scene,
                left.saturating_add(3),
                tail.saturating_sub(3),
                lane_width.saturating_sub(6).max(1),
                6,
                0x87e7ff,
            );
        }
    }
    if visible.contains(&head) {
        rect(
            scene,
            left.saturating_add(3),
            head.saturating_sub(4),
            lane_width.saturating_sub(6).max(1),
            8,
            0x74e5c5,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_font_fields_align_unicode_selection_caret_and_ime_underline() {
        use crate::font_atlas::FontAtlas;
        use std::sync::Arc;
        let atlas = Arc::new(
            FontAtlas::new(crate::font_fixture::font_bytes(), 14.0, 128, 128, 16).unwrap(),
        );
        let atlas = FontAtlas::extend_texts(&atlas, &["A가"]).unwrap();
        let texture = TextureId::allocate().unwrap();
        let font = FontText::new(atlas, texture).unwrap();
        let bounds = Bounds {
            x: 10,
            y: 10,
            width: 50,
            height: 32,
        };
        let mut editor = LineEditor::new("A가", 32).unwrap();
        editor.select_all();
        let mut scene = Scene::new(100, 100);
        text_field_with_font(&mut scene, &editor, bounds, true, Some(&font));
        scene.status().unwrap();
        assert_eq!(scene.rectangles().len(), 5);
        assert_eq!(scene.rectangles()[1].bounds, [18.0, 18.0, 17.0, 16.0]);
        assert_eq!(scene.rectangles()[4].bounds, [35.0, 18.0, 2.0, 16.0]);
        assert_eq!(
            scene
                .batches()
                .iter()
                .filter(|b| b.texture == texture)
                .map(|b| b.count)
                .sum::<u32>(),
            2
        );
        let preview = editor.preedit("가", Some((0, 3))).unwrap();
        scene.clear();
        text_field_with_font(&mut scene, &preview, bounds, true, Some(&font));
        assert_eq!(scene.rectangles()[1].bounds, [18.0, 18.0, 8.0, 16.0]);
        assert_eq!(scene.rectangles()[3].bounds, [18.0, 34.0, 8.0, 2.0]);
        assert_eq!(scene.rectangles()[4].bounds, [18.0, 18.0, 2.0, 16.0]);
        scene.clear();
        text_field_with_font(&mut scene, &preview, bounds, false, Some(&font));
        assert_eq!(scene.rectangles().len(), 2);
        scene.clear();
        text_field_with_font(
            &mut scene,
            &LineEditor::new("別", 32).unwrap(),
            bounds,
            true,
            Some(&font),
        );
        assert!(scene.status().is_err());
        assert!(scene.rectangles().is_empty());
    }
    #[test]
    fn committed_selection_clips_without_composition_and_hides_when_unfocused() {
        let mut editor = LineEditor::new("a별éz", 32).unwrap();
        editor.select_all();
        let bounds = Bounds {
            x: 10,
            y: 10,
            width: 40,
            height: 32,
        };
        let mut scene = Scene::new(100, 100);
        text_field(&mut scene, &editor, bounds, true);
        assert_eq!(scene.rectangles().len(), 5); // background, selection, 2 glyphs, caret
        assert_eq!(scene.rectangles()[1].bounds, [18.0, 18.0, 24.0, 16.0]);
        assert_eq!(scene.rectangles()[4].bounds, [42.0, 18.0, 2.0, 16.0]);
        assert_eq!(editor.visible_line(2).value, "éz");
        assert_eq!(editor.visible_line(2).composition, None);
        scene.clear();
        text_field(&mut scene, &editor, bounds, false);
        assert_eq!(scene.rectangles().len(), 3);
        assert_eq!(editor.selection(), Some((0, 7)));
        editor.clear_selection();
        scene.clear();
        text_field(&mut scene, &editor, bounds, true);
        assert_eq!(scene.rectangles().len(), 4);
    }
    #[test]
    fn composition_and_selection_use_scalar_cells_and_clip_to_the_visible_field() {
        let mut base = LineEditor::new("a별b", 32).unwrap();
        base.left();
        let editor = base.preedit("音é", Some((3, 5))).unwrap();
        let mut scene = Scene::new(100, 100);
        let mut bounds = Bounds {
            x: 10,
            y: 10,
            width: 76,
            height: 32,
        };
        text_field(&mut scene, &editor, bounds, true);
        let packets = scene.rectangles();
        assert_eq!(packets.len(), 9); // background, selection, 5 glyphs, underline, caret
        assert_eq!(packets[1].bounds, [54.0, 18.0, 12.0, 16.0]);
        assert_eq!(packets[7].bounds, [42.0, 34.0, 24.0, 2.0]);
        assert_eq!(packets[8].bounds, [54.0, 18.0, 2.0, 16.0]);
        bounds.width = 28; // One visible scalar cell containing é.
        scene.clear();
        text_field(&mut scene, &editor, bounds, true);
        assert_eq!(scene.rectangles().len(), 5);
        assert_eq!(scene.rectangles()[1].bounds, [18.0, 18.0, 12.0, 16.0]);
        assert_eq!(scene.rectangles()[3].bounds, [18.0, 34.0, 12.0, 2.0]);
        assert_eq!(scene.rectangles()[4].bounds, [18.0, 18.0, 2.0, 16.0]);
        for rectangle in scene.rectangles() {
            assert!(rectangle.bounds[0] >= 10.0);
            assert!(rectangle.bounds[0] + rectangle.bounds[2] <= 38.0);
        }
        assert_eq!((base.value(), base.cursor()), ("a별b", 4));
    }
    #[test]
    fn empty_selection_and_unfocused_fields_do_not_draw_selection_or_stale_underlines() {
        let base = LineEditor::new("", 32).unwrap();
        let bounds = Bounds {
            x: 10,
            y: 10,
            width: 76,
            height: 32,
        };
        let mut scene = Scene::new(100, 100);
        for selected in [None, Some((0, 0)), Some((3, 3))] {
            let editor = base.preedit("音é", selected).unwrap();
            scene.clear();
            text_field(&mut scene, &editor, bounds, true);
            assert_eq!(
                scene.rectangles().len(),
                if selected.is_some() { 5 } else { 4 }
            );
            assert_eq!(scene.rectangles()[3].bounds, [18.0, 34.0, 24.0, 2.0]);
            scene.clear();
            text_field(&mut scene, &editor, bounds, false);
            assert_eq!(scene.rectangles().len(), 3);
        }
        scene.clear();
        text_field(&mut scene, &base.preedit("", None).unwrap(), bounds, true);
        assert_eq!(scene.rectangles().len(), 2); // background + ordinary caret
        scene.clear();
        text_field(
            &mut scene,
            &base.preedit("音", Some((0, 3))).unwrap(),
            Bounds {
                width: 16,
                ..bounds
            },
            true,
        );
        assert_eq!(scene.rectangles().len(), 2); // no visible cells, no range geometry
    }
    #[test]
    fn fields_clip_glyph_count_and_keep_caret_in_bounds() {
        let bounds = Bounds {
            x: 10,
            y: 10,
            width: 40,
            height: 32,
        };
        let mut scene = Scene::new(100, 100);
        text_field_value(&mut scene, "abcdefgh", bounds);
        assert_eq!(scene.rectangles().len(), 3); // background + two glyphs
        scene.clear();
        let editor = LineEditor::new("abcdefgh", 32).unwrap();
        text_field(&mut scene, &editor, bounds, true);
        assert_eq!(scene.rectangles().len(), 4);
        let caret = scene.rectangles().last().unwrap();
        assert_eq!(caret.bounds, [42.0, 18.0, 2.0, 16.0]);
    }
    #[test]
    fn hold_remains_visible_after_head_passes_and_offscreen_instant_is_absent() {
        let mut scene = Scene::new(960, 720);
        note(&mut scene, (80, 160), 630, Some(200), 110..=625);
        assert_eq!(scene.rectangles().len(), 2);
        assert_eq!(scene.rectangles()[0].bounds, [86.0, 200.0, 68.0, 425.0]);
        scene.clear();
        note(&mut scene, (80, 160), 630, None, 110..=625);
        assert!(scene.rectangles().is_empty());
        note(
            &mut scene,
            (i64::MAX - 1, i64::MAX),
            i64::MIN,
            None,
            i64::MIN..=i64::MAX,
        );
        assert!(scene.rectangles().is_empty());
    }
}
