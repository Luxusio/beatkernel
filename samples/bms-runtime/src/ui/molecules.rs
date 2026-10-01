//! Small compositions of drawing atoms, reusable across screens.
use super::atoms::{rect, text};
use super::interaction::Bounds;
use super::text_input::LineEditor;
use crate::scene::Scene;
use std::ops::RangeInclusive;

/// A clipped line field; editing and focus belong to the menu owner.
pub fn text_field(scene: &mut Scene, editor: &LineEditor, bounds: Bounds, focused: bool) {
    if bounds.width < 16 || bounds.height < 30 {
        return;
    }
    let cells = usize::try_from((bounds.width - 16) / 12).unwrap_or(usize::MAX);
    let (visible, caret) = editor.visible(cells);
    paint_line(scene, visible, focused.then_some(caret), bounds);
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
    paint_line(scene, &value[..end], None, bounds);
}

fn paint_line(scene: &mut Scene, visible: &str, caret: Option<usize>, bounds: Bounds) {
    rect(
        scene,
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        if caret.is_some() { 0x354e6c } else { 0x263d59 },
    );
    let x = bounds.x.saturating_add(8);
    let y = bounds.y.saturating_add(8);
    text(
        scene,
        usize::try_from(x).unwrap_or(usize::MAX),
        usize::try_from(y).unwrap_or(usize::MAX),
        visible,
        2,
        0xf0f4ff,
    );
    if let Some(caret) = caret {
        let offset = i64::try_from(caret).unwrap_or(i64::MAX).saturating_mul(12);
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
