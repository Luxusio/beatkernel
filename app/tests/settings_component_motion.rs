#![cfg(feature = "graphics")]

use beatkernel_bms_runtime::{
    scene::{Scene, UiComponentKey, UiTransform},
    screen_lifecycle::ScreenInstanceId,
    settings::SettingsField,
    ui::{
        interaction::{Bounds, ControlId},
        layout::NodeId,
        settings::{SettingsFrame, SettingsView},
        text_input::LineEditor,
    },
};
use std::sync::Arc;

fn fields(count: usize) -> Vec<SettingsField> {
    (0..count)
        .map(|index| SettingsField {
            flag: "--bind",
            label: "KEY BINDING",
            hint: "BINDING HINT",
            value: format!("{index}:04"),
        })
        .collect()
}

fn update(view: &SettingsView, fields: &[SettingsField], selected: usize, pending: bool) {
    let editor = LineEditor::new(&fields[selected].value, 4096).unwrap();
    let profile = LineEditor::new("profile.json", 4096).unwrap();
    view.update(SettingsFrame {
        fields,
        selected,
        editor: &editor,
        profile: &profile,
        profile_focused: false,
        message: None,
        error: None,
        pending,
        hovered: None,
        armed: None,
    })
    .unwrap();
}

fn ordinary(view: &SettingsView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    (scene, hits)
}

fn regions(hits: &[(ControlId, Bounds)]) -> Vec<(u64, [i64; 4])> {
    hits.iter()
        .map(|(control, bounds)| (control.0, [bounds.x, bounds.y, bounds.width, bounds.height]))
        .collect()
}

#[test]
fn settings_targets_follow_displayed_page_and_loading_admission() {
    let view = SettingsView::new(ScreenInstanceId(411), 960, 720).unwrap();
    let data = fields(12);
    update(&view, &data, 0, false);
    let (_, hits) = ordinary(&view, 960, 720);
    assert!(hits.iter().any(|(control, _)| *control == ControlId(1000)));
    let first_slot = view.node_for_control(ControlId(1000)).unwrap().unwrap();
    assert_eq!(view.node_for_control(ControlId(1010)).unwrap(), None);
    assert_eq!(view.node_for_control(ControlId(9999)).unwrap(), None);

    update(&view, &data, 10, false);
    ordinary(&view, 960, 720);
    assert_eq!(view.node_for_control(ControlId(1000)).unwrap(), None);
    assert_eq!(
        view.node_for_control(ControlId(1010)).unwrap(),
        Some(first_slot)
    );
    assert_eq!(view.node_for_control(ControlId(1012)).unwrap(), None);

    update(&view, &data, 10, true);
    let (_, hits) = ordinary(&view, 960, 720);
    assert!(hits.is_empty());
    assert_eq!(view.node_for_control(ControlId(1010)).unwrap(), None);
    assert_eq!(view.node_for_control(ControlId(10)).unwrap(), None);
    assert_eq!(view.hit((25.0, 621.0)), None);
}

#[test]
fn settings_translated_button_uses_literal_pose_and_preserves_geometry() {
    let owner = ScreenInstanceId(412);
    let view = SettingsView::new(owner, 960, 720).unwrap();
    update(&view, &fields(1), 0, false);
    ordinary(&view, 960, 720);
    let node = view.node_for_control(ControlId(10)).unwrap().unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    let component = scene
        .component_id(UiComponentKey {
            screen: owner,
            node,
        })
        .unwrap();
    let identity = scene.geometry_stamp().0.clone();
    let revision = scene.geometry_stamp().1;
    let baseline_hits = hits.clone();
    for offset in [30.0, 30.5, 30.0] {
        let pose = UiTransform::new([offset, 0.0], [1.0, 1.0], 1.0).unwrap();
        scene
            .set_component_transforms(&[(component, pose)])
            .unwrap();
        assert_eq!(
            view.hit_components(&scene, owner, (55.0, 621.0)),
            Some(ControlId(10))
        );
        assert_eq!(view.hit_components(&scene, owner, (25.0, 621.0)), None);
        assert_eq!(
            view.hit_components(&scene, owner, (220.0, 621.0)),
            Some(ControlId(11))
        );
        assert_eq!(
            view.hit_components(&scene, ScreenInstanceId(999), (55.0, 621.0)),
            None
        );
        view.compose_components(&mut scene, &mut hits, owner, &[node])
            .unwrap();
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(revision, scene.geometry_stamp().1);
        assert_eq!(regions(&baseline_hits), regions(&hits));
        assert_eq!(scene.component_transform(component), Some(pose));
    }
    scene
        .set_component_transforms(&[(
            component,
            UiTransform::new([30.0, 0.0], [1.0, 1.0], 0.0).unwrap(),
        )])
        .unwrap();
    assert_eq!(view.hit_components(&scene, owner, (55.0, 621.0)), None);
    assert_eq!(view.hit((25.0, 621.0)), Some(ControlId(10)));
    assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
    assert_eq!(revision, scene.geometry_stamp().1);
}

#[test]
fn settings_cropped_source_can_reenter_fixed_viewport_clip() {
    let owner = ScreenInstanceId(413);
    let mut view = SettingsView::new(owner, 960, 720).unwrap();
    update(&view, &fields(1), 0, false);
    ordinary(&view, 960, 720);
    let audio = view.node_for_control(ControlId(16)).unwrap().unwrap();
    view.resize(480, 360).unwrap();
    let (_, original_hits) = ordinary(&view, 480, 360);
    assert_eq!(view.node_for_control(ControlId(16)).unwrap(), None);
    let mut scene = Scene::new(480, 360);
    let mut hits = Vec::new();
    view.compose_components(&mut scene, &mut hits, owner, &[audio])
        .unwrap();
    let component = scene
        .component_id(UiComponentKey {
            screen: owner,
            node: audio,
        })
        .unwrap();
    scene
        .set_component_transforms(&[(
            component,
            UiTransform::new([-300.0, 0.0], [1.0, 1.0], 1.0).unwrap(),
        )])
        .unwrap();
    // The row anchor halves to x=132, but its child offsets remain literal.
    // AUDIO is x=676..801 before motion, hence x=376..501 afterwards.
    assert_eq!(
        view.hit_components(&scene, owner, (400.0, 31.0)),
        Some(ControlId(16))
    );
    assert_eq!(
        view.hit_components(&scene, owner, (479.999, 59.999)),
        Some(ControlId(16))
    );
    // The first editor starts at y=60 after this resize and paints later.
    // Its overlap with the moved button must retain ordinary painter order.
    assert_eq!(
        view.hit_components(&scene, owner, (479.999, 63.999)),
        Some(ControlId(1000))
    );
    assert_eq!(view.hit_components(&scene, owner, (480.0, 31.0)), None);
    assert_ne!(
        view.hit_components(&scene, owner, (400.0, 64.0)),
        Some(ControlId(16))
    );
    assert_eq!(view.hit_components(&scene, owner, (f64::NAN, 31.0)), None);
    let (_, restored_hits) = ordinary(&view, 480, 360);
    assert_eq!(regions(&original_hits), regions(&restored_hits));
    assert_eq!(view.node_for_control(ControlId(16)).unwrap(), None);
}

#[test]
fn settings_invalid_component_requests_preserve_existing_scene_and_hits() {
    let owner = ScreenInstanceId(414);
    let view = SettingsView::new(owner, 960, 720).unwrap();
    update(&view, &fields(1), 0, false);
    ordinary(&view, 960, 720);
    let node = view.node_for_control(ControlId(10)).unwrap().unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    let component = scene
        .component_id(UiComponentKey {
            screen: owner,
            node,
        })
        .unwrap();
    let pose = UiTransform::new([20.0, 0.0], [1.0, 1.0], 1.0).unwrap();
    scene
        .set_component_transforms(&[(component, pose)])
        .unwrap();
    let identity = scene.geometry_stamp().0.clone();
    let revision = scene.geometry_stamp().1;
    let baseline_hits = hits.clone();
    for (screen, nodes) in [
        (owner, vec![node, node]),
        (owner, vec![NodeId(1023)]),
        (ScreenInstanceId(0), vec![node]),
    ] {
        assert!(view
            .compose_components(&mut scene, &mut hits, screen, &nodes)
            .is_err());
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(revision, scene.geometry_stamp().1);
        assert_eq!(regions(&hits), regions(&baseline_hits));
        assert_eq!(scene.component_transform(component), Some(pose));
        assert_eq!(
            view.hit_components(&scene, owner, (45.0, 621.0)),
            Some(ControlId(10))
        );
    }
}
