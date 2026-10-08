//! Actual Settings packets, mounted geometry, native drafts and capability fields.
use super::*;
use crate::{
    settings::{NativeSettings, SettingsHost},
    ui::layout::{LayoutChange, LayoutUpdate, NodeId},
};

fn frame<'a>(
    fields: &'a [SettingsField],
    editor: &'a LineEditor,
    profile: &'a LineEditor,
) -> SettingsFrame<'a> {
    SettingsFrame {
        fields,
        selected: 0,
        editor,
        profile,
        profile_focused: false,
        message: None,
        error: None,
        pending: false,
        hovered: None,
        armed: None,
    }
}

fn compose(view: &SettingsView, extent: [u32; 2]) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::with_capacity(extent[0], extent[1], 64);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    (scene, hits)
}

fn shape(bounds: Bounds) -> [i64; 4] {
    [bounds.x, bounds.y, bounds.width, bounds.height]
}

fn paint(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.color, rectangle.uv))
        .collect()
}

fn node(view: &SettingsView, matches: impl Fn(Component) -> bool) -> NodeId {
    view.layout
        .leaves()
        .iter()
        .find(|leaf| matches(leaf.component))
        .unwrap()
        .id
}

#[test]
fn mounted_settings_keeps_every_native_schema_page_and_original_control_order() {
    for host in [
        SettingsHost::Linux,
        SettingsHost::Windows,
        SettingsHost::Macos,
    ] {
        let values = NativeSettings::from_args(&[], host).unwrap();
        let original = values.fields().to_vec();
        let view = SettingsView::new(ScreenInstanceId(41), 960, 720).unwrap();
        let profile = LineEditor::new("설정/profile.bkp", 4096).unwrap();
        let identities = view.nodes.identities();
        for first in (0..values.fields().len()).step_by(10) {
            let editor = LineEditor::new(&values.fields()[first].value, 4096).unwrap();
            let mut update = frame(values.fields(), &editor, &profile);
            update.selected = first;
            view.update(update).unwrap();
            let (scene, hits) = compose(&view, [960, 720]);
            let count = (values.fields().len() - first).min(10);
            let expected: Vec<_> = [74, 19, 18, 17, 16]
                .into_iter()
                .chain((first..first + count).map(|index| 1000 + index as u64))
                .chain([15, 10, 11, 12, 13, 14])
                .collect();
            assert_eq!(
                hits.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
                expected
            );
            for &(id, expected_bounds, _) in &BUTTONS {
                let bounds = hits.iter().find(|(actual, _)| *actual == id).unwrap().1;
                assert_eq!(shape(bounds), shape(expected_bounds));
                assert_eq!(
                    view.hit((bounds.x as f64 + 1.0, bounds.y as f64 + 1.0)),
                    Some(id)
                );
                assert!(scene.rectangles().iter().any(|rectangle| rectangle.bounds
                    == [
                        bounds.x as f32,
                        bounds.y as f32,
                        bounds.width as f32,
                        bounds.height as f32
                    ]));
            }
            for slot in 0..count {
                assert_eq!(
                    shape(hits[5 + slot].1),
                    [280, 120 + slot as i64 * 39, 650, 32]
                );
            }
            assert_eq!(view.id(), ScreenInstanceId(41));
            assert_eq!(view.nodes.identities(), identities);
            let before = view.nodes.paints();
            let mut repeated = frame(values.fields(), &editor, &profile);
            repeated.selected = first;
            view.update(repeated).unwrap();
            assert!(!view.dirty());
            assert_eq!(view.nodes.paints(), before);
        }
        assert_eq!(values.fields(), original);
    }
}

#[test]
fn narrower_real_output_capability_is_not_expanded_into_missing_settings() {
    let args = vec![
        "--alsa".to_owned(),
        "null".to_owned(),
        "--period-frames".to_owned(),
        "64".to_owned(),
    ];
    let capability = NativeSettings::output_capability(&args, SettingsHost::Linux).unwrap();
    assert_eq!(capability.fields().len(), 2);
    let view = SettingsView::new(ScreenInstanceId(42), 960, 720).unwrap();
    let editor = LineEditor::new("null", 4096).unwrap();
    let profile = LineEditor::new("", 4096).unwrap();
    view.update(frame(capability.fields(), &editor, &profile))
        .unwrap();
    let (_, hits) = compose(&view, [960, 720]);
    assert_eq!(
        hits.iter()
            .filter(|(id, _)| id.0 >= 1000)
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>(),
        [1000, 1001]
    );
    assert_eq!(view.count.get_untracked(), 2);
    assert!(view.fields[2].get_untracked().is_none());
}

#[test]
fn edited_draft_profile_preedit_pending_status_and_refusal_preserve_real_state() {
    let original = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let mut draft = original.clone();
    let selected = draft
        .fields()
        .iter()
        .position(|field| field.flag == "--chart-seed")
        .unwrap();
    let profile = LineEditor::new("설정/profile.bkp", 4096).unwrap();
    let mut editor = LineEditor::new("37", 4096).unwrap();
    editor.select_all();
    let preview = editor.preedit("별é", Some((0, 5))).unwrap();
    let view = SettingsView::new(ScreenInstanceId(43), 960, 720).unwrap();
    let mut update = frame(draft.fields(), &preview, &profile);
    update.selected = selected;
    view.update(update).unwrap();
    let identities = view.nodes.identities();
    let (before, before_hits) = compose(&view, [960, 720]);
    assert_eq!(
        draft.fields(),
        original.fields(),
        "visual preedit never commits the business draft"
    );
    assert_eq!(view.editor.get_untracked(), preview);
    let before_paints = view.nodes.paints();
    let mut bad = frame(draft.fields(), &editor, &profile);
    bad.selected = draft.fields().len();
    bad.pending = true;
    bad.error = Some("invalid frame must not publish");
    assert!(view.update(bad).is_err());
    assert_eq!(view.nodes.paints(), before_paints);
    let (unchanged, unchanged_hits) = compose(&view, [960, 720]);
    assert_eq!(paint(&unchanged), paint(&before));
    assert_eq!(
        unchanged_hits
            .iter()
            .map(|(id, b)| (*id, shape(*b)))
            .collect::<Vec<_>>(),
        before_hits
            .iter()
            .map(|(id, b)| (*id, shape(*b)))
            .collect::<Vec<_>>()
    );
    assert!(draft.set_value(selected, &"x".repeat(4097)).is_err());
    assert_eq!(draft.fields(), original.fields());
    draft.set_value(selected, editor.value()).unwrap();
    assert_eq!(draft.chart_seed().unwrap(), 37);
    assert_eq!(original.chart_seed().unwrap(), 0);
    let mut pending = frame(draft.fields(), &editor, &profile);
    pending.selected = selected;
    pending.profile_focused = true;
    pending.pending = true;
    pending.hovered = Some(ControlId(10));
    pending.armed = Some(ControlId(11));
    pending.message = Some("SAVED");
    pending.error = Some("device refusal");
    view.update(pending).unwrap();
    let (loading, hits) = compose(&view, [960, 720]);
    assert!(hits.is_empty());
    assert_eq!(view.hit((25.0, 621.0)), None);
    assert_eq!(view.profile.get_untracked(), profile);
    assert!(view.profile_focused.get_untracked());
    assert_ne!(paint(&loading), paint(&before));
    // Cancel/back restores the caller-owned committed draft; Apply's original
    // control identity stays available when the owner publishes a ready frame.
    let restored = LineEditor::new(&original.fields()[selected].value, 4096).unwrap();
    let mut ready = frame(original.fields(), &restored, &profile);
    ready.selected = selected;
    view.update(ready).unwrap();
    let (_, hits) = compose(&view, [960, 720]);
    assert!(hits.iter().any(|(id, _)| *id == ControlId(10)));
    assert!(hits.iter().any(|(id, _)| *id == ControlId(11)));
    assert_eq!(view.editor.get_untracked(), restored);
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn actual_extent_updates_share_paint_and_hit_clips_suspend_and_restore_without_remount() {
    let values = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let editor = LineEditor::new("original draft", 4096).unwrap();
    let profile = LineEditor::new("profile.bkp", 4096).unwrap();
    let mut view = SettingsView::new(ScreenInstanceId(44), 960, 720).unwrap();
    view.update(frame(values.fields(), &editor, &profile))
        .unwrap();
    let (original, original_hits) = compose(&view, [960, 720]);
    let identities = view.nodes.identities();
    let paints = view.nodes.paints();
    assert!(!view.resize(960, 720).unwrap());
    assert_eq!(view.nodes.paints(), paints);
    assert!(!view.dirty());
    assert!(view.resize(700, 500).unwrap());
    let (smaller, hits) = compose(&view, [700, 500]);
    assert_ne!(paint(&smaller), paint(&original));
    for &(id, bounds) in &hits {
        assert!(
            bounds.x >= 0
                && bounds.y >= 0
                && bounds.x + bounds.width <= 700
                && bounds.y + bounds.height <= 500
        );
        let point = (bounds.x as f64 + 0.5, bounds.y as f64 + 0.5);
        let expected = hits
            .iter()
            .rev()
            .find(|(_, b)| b.contains(point))
            .unwrap()
            .0;
        assert_eq!(
            view.hit(point),
            Some(expected),
            "retained admission agrees with composed order for {id:?}"
        );
    }
    for rectangle in smaller.rectangles() {
        assert!(rectangle.bounds[0] >= 0.0 && rectangle.bounds[1] >= 0.0);
        assert!(
            rectangle.bounds[0] + rectangle.bounds[2] <= 700.0
                && rectangle.bounds[1] + rectangle.bounds[3] <= 500.0
        );
    }
    assert!(view.resize(0, 500).unwrap());
    let (suspended, hits) = compose(&view, [0, 500]);
    assert!(hits.is_empty() && suspended.rectangles().is_empty());
    assert_eq!(view.hit((1.0, 1.0)), None);
    assert_eq!(view.editor.get_untracked(), editor);
    assert_eq!(view.profile.get_untracked(), profile);
    assert!(!view.resize(0, 500).unwrap());
    assert!(view.resize(960, 720).unwrap());
    let (restored, hits) = compose(&view, [960, 720]);
    assert_eq!(paint(&restored), paint(&original));
    assert_eq!(
        hits.iter()
            .map(|(id, b)| (*id, shape(*b)))
            .collect::<Vec<_>>(),
        original_hits
            .iter()
            .map(|(id, b)| (*id, shape(*b)))
            .collect::<Vec<_>>()
    );
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn mounted_child_reflow_and_local_clip_update_actual_dependent_packets_atomically() {
    let values = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let editor = LineEditor::new("draft", 4096).unwrap();
    let profile = LineEditor::new("", 4096).unwrap();
    let mut view = SettingsView::new(ScreenInstanceId(45), 960, 720).unwrap();
    view.update(frame(values.fields(), &editor, &profile))
        .unwrap();
    compose(&view, [960, 720]);
    let identities = view.nodes.identities();
    let apply = node(&view, |component| {
        matches!(component, Component::Action(ControlId(10), _))
    });
    let back = node(&view, |component| {
        matches!(component, Component::Action(ControlId(11), _))
    });
    let old_back = view.layout.geometry(back).unwrap().bounds;
    let mut candidate = view.layout.clone();
    candidate
        .update(&[LayoutUpdate {
            id: apply,
            change: LayoutChange::Size([160, 34]),
        }])
        .unwrap();
    view.nodes.relayout(&candidate).unwrap();
    view.layout = candidate;
    let (_, hits) = compose(&view, [960, 720]);
    let moved = hits.iter().find(|(id, _)| *id == ControlId(11)).unwrap().1;
    assert_eq!(moved.x, old_back.x - 10);
    assert_eq!(moved.y, old_back.y);
    assert_eq!(
        view.hit((moved.x as f64 + 1.0, moved.y as f64 + 1.0)),
        Some(ControlId(11))
    );
    let before = view.nodes.paints();
    let mut refused = view.layout.clone();
    assert!(refused
        .update(&[LayoutUpdate {
            id: apply,
            change: LayoutChange::Size([-1, 34])
        }])
        .is_err());
    assert_eq!(view.nodes.paints(), before);
    assert_eq!(
        shape(view.layout.geometry(back).unwrap().bounds),
        shape(moved)
    );
    let mut clipped = view.layout.clone();
    clipped
        .update(&[LayoutUpdate {
            id: NodeId(0),
            change: LayoutChange::Clip(Some(Bounds {
                x: 0,
                y: 0,
                width: 300,
                height: 150,
            })),
        }])
        .unwrap();
    view.nodes.relayout(&clipped).unwrap();
    view.layout = clipped;
    let (scene, hits) = compose(&view, [960, 720]);
    let field = hits
        .iter()
        .find(|(id, _)| *id == ControlId(1000))
        .unwrap()
        .1;
    assert_eq!(shape(field), [280, 120, 20, 30]);
    assert_eq!(view.hit((299.5, 149.5)), Some(ControlId(1000)));
    assert_eq!(view.hit((300.0, 130.0)), None);
    assert_eq!(view.hit((290.0, 150.0)), None);
    assert!(scene
        .rectangles()
        .iter()
        .all(|r| r.bounds[0] + r.bounds[2] <= 300.0 && r.bounds[1] + r.bounds[3] <= 150.0));
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn restoration_reuses_packets_and_disposal_releases_settings_subscriptions() {
    let values = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let editor = LineEditor::new("draft", 4096).unwrap();
    let profile = LineEditor::new("profile", 4096).unwrap();
    let view = SettingsView::new(ScreenInstanceId(46), 960, 720).unwrap();
    view.update(frame(values.fields(), &editor, &profile))
        .unwrap();
    let (mut scene, mut hits) = compose(&view, [960, 720]);
    let original = paint(&scene);
    let identities = view.nodes.identities();
    let paints = view.nodes.paints();
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    assert_eq!(paint(&scene), original);
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(view.nodes.paints(), paints);
    let weak = view.nodes.weak_dirty();
    assert!(weak.upgrade().is_some());
    drop(view);
    assert!(weak.upgrade().is_none());
}
