//! Small compositions of drawing atoms, reusable across screens.
use super::atoms::{rect, text};
use super::interaction::Bounds;
use crate::scene::Scene;
use std::ops::RangeInclusive;

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
