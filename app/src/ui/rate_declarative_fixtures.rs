use super::*;

fn rate(n: i64, d: u64) -> beatkernel::transport::Rate {
    beatkernel::transport::Rate::new(n, d).unwrap()
}

fn frame(numerator: &str, denominator: &str) -> RateFrame {
    RateFrame {
        numerator: LineEditor::new(numerator, 64).unwrap(),
        denominator: LineEditor::new(denominator, 64).unwrap(),
        focused: None,
        pending: false,
        applied: Some(crate::practice_control::PracticeRateState {
            revision: 7,
            rate: beatkernel::transport::Rate::NORMAL,
        }),
        error: None,
        hovered: None,
        armed: None,
    }
}

fn compose(view: &RateView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    (scene, hits)
}

fn geometry(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|r| (r.bounds, r.color, r.uv))
        .collect()
}

fn regions(hits: &[(ControlId, Bounds)]) -> Vec<(ControlId, [i64; 4])> {
    hits.iter()
        .map(|(id, b)| (*id, [b.x, b.y, b.width, b.height]))
        .collect()
}

fn rgba(color: u32) -> [f32; 4] {
    [
        ((color >> 16) & 255) as f32 / 255.0,
        ((color >> 8) & 255) as f32 / 255.0,
        (color & 255) as f32 / 255.0,
        1.0,
    ]
}

fn assert_editor_focus(
    scene: &Scene,
    hits: &[(ControlId, Bounds)],
    control: ControlId,
    focused: bool,
    caret_column: i64,
) {
    let bounds = hits.iter().find(|(id, _)| *id == control).unwrap().1;
    let background = scene
        .rectangles()
        .iter()
        .filter(|rectangle| {
            rectangle.bounds == [bounds.x, bounds.y, bounds.width, bounds.height].map(|n| n as f32)
        })
        .collect::<Vec<_>>();
    assert_eq!(background.len(), 1, "one editor background for {control:?}");
    assert_eq!(
        background[0].color,
        rgba(if focused { 0x354e6c } else { 0x263d59 }),
        "editor focus background for {control:?}"
    );
    let carets = scene
        .rectangles()
        .iter()
        .filter(|rectangle| {
            let [x, y, width, height] = rectangle.bounds;
            rectangle.color == rgba(0x74e5c5)
                && x >= bounds.x as f32
                && y >= bounds.y as f32
                && x + width <= (bounds.x + bounds.width) as f32
                && y + height <= (bounds.y + bounds.height) as f32
        })
        .map(|rectangle| rectangle.bounds)
        .collect::<Vec<_>>();
    let expected = if focused {
        vec![[bounds.x + 8 + 12 * caret_column, bounds.y + 8, 2, 16].map(|n| n as f32)]
    } else {
        Vec::new()
    };
    assert_eq!(carets, expected, "editor caret for {control:?}");
}

#[test]
fn optional_focus_paints_only_the_actual_editor_and_clears_both_for_apply_or_none() {
    let view = RateView::new(ScreenInstanceId(417), 960, 720).unwrap();
    let mut state = frame("12", "8");
    view.update(state.clone());
    let original = compose(&view, 960, 720);
    let identities = view.nodes.identities();
    let original_hits = regions(&original.1);
    for (focused, expected_repaints) in [
        (None, 0),
        (Some(NUMERATOR_CONTROL), 1),
        (Some(DENOMINATOR_CONTROL), 2),
        (Some(APPLY_CONTROL), 1),
        (None, 0),
    ] {
        let before = view.nodes.paints();
        state.focused = focused;
        view.update(state.clone());
        let (scene, hits) = compose(&view, 960, 720);
        assert_editor_focus(
            &scene,
            &hits,
            NUMERATOR_CONTROL,
            focused == Some(NUMERATOR_CONTROL),
            2,
        );
        assert_editor_focus(
            &scene,
            &hits,
            DENOMINATOR_CONTROL,
            focused == Some(DENOMINATOR_CONTROL),
            1,
        );
        assert_eq!(regions(&hits), original_hits);
        assert_eq!(view.nodes.identities(), identities);
        let paints = view.nodes.paints();
        assert_eq!(
            paints.iter().zip(before).filter(|(a, b)| **a != *b).count(),
            expected_repaints,
            "focus {focused:?} must preserve unrelated retained packets"
        );
        if focused.is_none() || focused == Some(APPLY_CONTROL) {
            assert_eq!(geometry(&scene), geometry(&original.0));
        }
        view.update(state.clone());
        assert!(!view.dirty());
        let idle = compose(&view, 960, 720);
        assert_eq!(geometry(&idle.0), geometry(&scene));
        assert_eq!(view.nodes.paints(), paints);
    }
}

#[test]
fn retained_rate_model_is_explicit_and_controls_have_distinct_stable_nodes_and_hits() {
    let owner = ScreenInstanceId(410);
    let view = RateView::new(owner, 960, 720).unwrap();
    view.update(frame("6", "4"));
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(view.id(), owner);
    assert!(!scene.rectangles().is_empty());
    let mut nodes = Vec::new();
    for control in [NUMERATOR_CONTROL, DENOMINATOR_CONTROL, APPLY_CONTROL] {
        assert_eq!(hits.iter().filter(|(id, _)| *id == control).count(), 1);
        let node = view.node_for_control(control).unwrap().unwrap();
        assert!(!nodes.contains(&node));
        nodes.push(node);
        let bounds = hits.iter().find(|(id, _)| *id == control).unwrap().1;
        assert!(bounds.width > 0 && bounds.height > 0);
        assert!(bounds.x >= 0 && bounds.y >= 0);
        assert!(bounds.x + bounds.width <= 960);
        assert!(bounds.y + bounds.height <= 720);
        assert_eq!(
            view.hit((bounds.x as f64 + 0.5, bounds.y as f64 + 0.5)),
            Some(control)
        );
    }
    assert_ne!(NUMERATOR_CONTROL, DENOMINATOR_CONTROL);
    assert_ne!(NUMERATOR_CONTROL, APPLY_CONTROL);
    assert_ne!(DENOMINATOR_CONTROL, APPLY_CONTROL);
    assert_eq!(view.node_for_control(ControlId(u64::MAX)).unwrap(), None);
}

#[test]
fn unchanged_frames_and_idle_composition_do_not_repaint_or_rebuild_retained_packets() {
    let view = RateView::new(ScreenInstanceId(411), 960, 720).unwrap();
    let state = frame("3", "2");
    view.update(state.clone());
    let original = compose(&view, 960, 720);
    assert!(!view.dirty());
    let identities = view.nodes.identities();
    let paints = view.nodes.paints();
    for _ in 0..16 {
        view.update(state.clone());
        assert!(!view.dirty());
        let idle = compose(&view, 960, 720);
        assert_eq!(geometry(&idle.0), geometry(&original.0));
        assert_eq!(regions(&idle.1), regions(&original.1));
        assert_eq!(view.nodes.identities(), identities);
        assert_eq!(view.nodes.paints(), paints);
    }
}

#[test]
fn pending_disables_apply_hit_without_rewriting_draft_or_applied_state() {
    let view = RateView::new(ScreenInstanceId(412), 960, 720).unwrap();
    let mut state = frame("6", "4");
    state.focused = Some(DENOMINATOR_CONTROL);
    view.update(state.clone());
    let before = compose(&view, 960, 720);
    let apply = before
        .1
        .iter()
        .find(|(id, _)| *id == APPLY_CONTROL)
        .unwrap()
        .1;
    let applied = state.applied;
    state.pending = true;
    view.update(state.clone());
    let pending = compose(&view, 960, 720);
    assert!(pending.1.iter().all(|(id, _)| *id != APPLY_CONTROL));
    assert_ne!(
        view.hit((apply.x as f64 + 0.5, apply.y as f64 + 0.5)),
        Some(APPLY_CONTROL)
    );
    for control in [NUMERATOR_CONTROL, DENOMINATOR_CONTROL] {
        let original = before.1.iter().find(|(id, _)| *id == control).unwrap();
        let retained = pending.1.iter().find(|(id, _)| *id == control).unwrap();
        assert_eq!(regions(&[*original]), regions(&[*retained]));
    }
    assert_eq!(state.numerator.value(), "6");
    assert_eq!(state.denominator.value(), "4");
    assert_eq!(state.applied, applied);
    state.pending = false;
    view.update(state);
    let restored = compose(&view, 960, 720);
    assert_eq!(geometry(&restored.0), geometry(&before.0));
    assert_eq!(regions(&restored.1), regions(&before.1));
}

#[test]
fn cursor_focus_errors_and_actual_rate_each_update_only_affected_retained_packets() {
    let view = RateView::new(ScreenInstanceId(413), 960, 720).unwrap();
    let mut state = frame("12", "8");
    state.focused = Some(NUMERATOR_CONTROL);
    view.update(state.clone());
    compose(&view, 960, 720);
    let identities = view.nodes.identities();
    let assert_partial = |before: Vec<usize>| {
        let after = view.nodes.paints();
        assert_eq!(after.len(), before.len());
        let changed = after.iter().zip(before).filter(|(a, b)| **a != *b).count();
        assert!(changed > 0, "changed presentation must repaint");
        assert!(
            changed < after.len(),
            "a local change must preserve unrelated packets"
        );
        assert_eq!(view.nodes.identities(), identities);
    };
    let before = view.nodes.paints();
    state.numerator.left();
    view.update(state.clone());
    assert_partial(before);
    let before = view.nodes.paints();
    state.focused = Some(DENOMINATOR_CONTROL);
    view.update(state.clone());
    assert_partial(before);
    let before = view.nodes.paints();
    state.error = Some("rate denominator must be positive".into());
    view.update(state.clone());
    assert_partial(before);
    let with_error = compose(&view, 960, 720);
    let before = view.nodes.paints();
    state.applied = Some(crate::practice_control::PracticeRateState {
        revision: 8,
        rate: rate(3, 2),
    });
    view.update(state.clone());
    assert_partial(before);
    assert_ne!(
        geometry(&compose(&view, 960, 720).0),
        geometry(&with_error.0)
    );
    assert_eq!(state.numerator.value(), "12");
    assert_eq!(state.denominator.value(), "8");
    assert_eq!(state.focused, Some(DENOMINATOR_CONTROL));
}

#[test]
fn applied_revision_is_visible_even_when_the_reduced_rate_is_unchanged() {
    let view = RateView::new(ScreenInstanceId(414), 960, 720).unwrap();
    let mut state = frame("4", "4");
    view.update(state.clone());
    let before = geometry(&compose(&view, 960, 720).0);
    state.applied.as_mut().unwrap().revision += 1;
    view.update(state);
    let after = geometry(&compose(&view, 960, 720).0);
    assert_ne!(
        before, after,
        "actual revision belongs in the applied presentation"
    );
}

#[test]
fn cropped_controls_use_the_same_geometry_for_rendering_and_finite_half_open_hit_tests() {
    let mut view = RateView::new(ScreenInstanceId(415), 960, 720).unwrap();
    view.update(frame("3", "2"));
    let identities = view.nodes.identities();
    for (width, height) in [(1200, 800), (480, 360), (19, 13), (1, 1)] {
        assert!(view.resize(width, height).unwrap());
        let (scene, hits) = compose(&view, width, height);
        for rectangle in scene.rectangles() {
            let [x, y, w, h] = rectangle.bounds;
            assert!(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite());
            assert!(x >= 0.0 && y >= 0.0 && w >= 0.0 && h >= 0.0);
            assert!(x + w <= width as f32 && y + h <= height as f32);
        }
        for (_, bounds) in &hits {
            assert!(bounds.x >= 0 && bounds.y >= 0 && bounds.width > 0 && bounds.height > 0);
            assert!(bounds.x + bounds.width <= i64::from(width));
            assert!(bounds.y + bounds.height <= i64::from(height));
            for point in [
                (bounds.x as f64, bounds.y as f64),
                (
                    (bounds.x + bounds.width) as f64 - 0.001,
                    (bounds.y + bounds.height) as f64 - 0.001,
                ),
                ((bounds.x + bounds.width) as f64, bounds.y as f64),
                (bounds.x as f64, (bounds.y + bounds.height) as f64),
            ] {
                assert_eq!(
                    view.hit(point),
                    hits.iter()
                        .rev()
                        .find(|(_, b)| b.contains(point))
                        .map(|(id, _)| *id)
                );
            }
        }
        assert_eq!(view.hit((f64::NAN, 0.0)), None);
        assert_eq!(view.hit((0.0, f64::INFINITY)), None);
        assert_eq!(view.hit((-0.001, 0.0)), None);
        assert_eq!(view.hit((f64::from(width), f64::from(height))), None);
        assert_eq!(view.nodes.identities(), identities);
        assert!(!view.resize(width, height).unwrap());
    }
}

#[test]
fn zero_extent_hides_child_and_restores_exact_draft_focus_error_and_node_identity() {
    let mut view = RateView::new(ScreenInstanceId(416), 960, 720).unwrap();
    let mut state = frame("invalid", "0");
    state.focused = Some(DENOMINATOR_CONTROL);
    state.error = Some("invalid positive rate".into());
    view.update(state);
    let before = compose(&view, 960, 720);
    let identities = view.nodes.identities();
    for extent in [(0, 720), (960, 0), (0, 0)] {
        view.resize(extent.0, extent.1).unwrap();
        let hidden = compose(&view, 960, 720);
        assert!(hidden.0.rectangles().is_empty());
        assert!(hidden.1.is_empty());
        assert_eq!(view.hit((0.0, 0.0)), None);
    }
    view.resize(960, 720).unwrap();
    let restored = compose(&view, 960, 720);
    assert_eq!(geometry(&restored.0), geometry(&before.0));
    assert_eq!(regions(&restored.1), regions(&before.1));
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn play_lifecycle_disposes_all_rate_reactive_nodes_and_reentry_uses_fresh_identity() {
    use crate::screen_lifecycle::{ScreenNavigator, ScreenRoute};
    let mut navigator = ScreenNavigator::default();
    navigator
        .navigate(ScreenRoute::Play { replay: false }, false, true)
        .unwrap();
    let original_id = navigator.active_id().unwrap();
    let view = RateView::new(original_id, 960, 720).unwrap();
    let mut state = frame("7", "3");
    state.pending = true;
    view.update(state);
    compose(&view, 960, 720);
    let weak = view.nodes.weak_dirty();
    navigator
        .navigate(ScreenRoute::Results { replay: false }, false, true)
        .unwrap();
    assert!(navigator.retains(original_id));
    navigator.back(false, true).unwrap();
    assert!(!navigator.retains(original_id));
    drop(view);
    assert!(
        weak.upgrade().is_none(),
        "leaving Play releases retained reactive ownership"
    );
    navigator
        .navigate(ScreenRoute::Play { replay: false }, false, true)
        .unwrap();
    let next_id = navigator.active_id().unwrap();
    assert_ne!(next_id, original_id);
    let next = RateView::new(next_id, 960, 720).unwrap();
    next.update(frame("1", "1"));
    let (_, hits) = compose(&next, 960, 720);
    assert!(hits.iter().any(|(id, _)| *id == APPLY_CONTROL));
}
