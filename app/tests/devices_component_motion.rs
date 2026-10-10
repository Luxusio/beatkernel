#![cfg(feature = "graphics")]

use beatkernel_bms_runtime::{
    device_catalog::{DeviceCatalog, DeviceChoice, DeviceRequest},
    local_players::PlayerId,
    local_setup::{BrowserInputKind, BrowserInputSource},
    scene::{Scene, UiComponentKey, UiTransform},
    screen_lifecycle::ScreenInstanceId,
    ui::{
        devices::{BrowserDevicesFrame, DevicesFrame, DevicesView},
        interaction::{Bounds, ControlId},
        layout::NodeId,
    },
};
use std::sync::Arc;

fn catalog(count: usize) -> DeviceCatalog {
    DeviceCatalog::new(
        DeviceRequest::LinuxKeyboard,
        (0..count)
            .map(|index| DeviceChoice {
                id: format!("/dev/input/event{index}"),
                label: format!("KEYBOARD {index}"),
                detail: "ORIGINAL DEVICE METADATA".into(),
                selectable: index % 2 == 0,
            })
            .collect(),
    )
    .unwrap()
}

fn native(view: &DevicesView, catalog: &DeviceCatalog, first: usize, pending: bool) {
    view.update(DevicesFrame {
        catalog,
        player: Some(PlayerId(7)),
        selected: Some(first),
        first,
        pending,
        error: None,
        hovered: None,
        armed: None,
    })
    .unwrap();
}

fn ordinary(view: &DevicesView) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    (scene, hits)
}

fn hit_geometry(hits: &[(ControlId, Bounds)]) -> Vec<(u64, i64, i64, i64, i64)> {
    hits.iter()
        .map(|(id, bounds)| (id.0, bounds.x, bounds.y, bounds.width, bounds.height))
        .collect()
}

#[test]
fn native_device_targets_follow_inventory_page_and_selection_admission() {
    let view = DevicesView::new(ScreenInstanceId(441), 960, 720).unwrap();
    let data = catalog(12);
    native(&view, &data, 0, false);
    ordinary(&view);
    let first_slot = view.node_for_control(ControlId(10000)).unwrap().unwrap();
    assert_eq!(view.node_for_control(ControlId(10001)).unwrap(), None);
    assert_eq!(view.node_for_control(ControlId(10010)).unwrap(), None);
    assert!(view.node_for_control(ControlId(20)).unwrap().is_some());
    // Native previous/next remain legacy admitted controls even at an edge.
    assert!(view.node_for_control(ControlId(23)).unwrap().is_some());
    assert!(view.node_for_control(ControlId(24)).unwrap().is_some());

    native(&view, &data, 10, false);
    ordinary(&view);
    assert_eq!(view.node_for_control(ControlId(10000)).unwrap(), None);
    assert_eq!(
        view.node_for_control(ControlId(10010)).unwrap(),
        Some(first_slot)
    );
    assert_eq!(view.node_for_control(ControlId(10011)).unwrap(), None);
    assert!(view.node_for_control(ControlId(23)).unwrap().is_some());
    assert!(view.node_for_control(ControlId(24)).unwrap().is_some());

    native(&view, &data, 10, true);
    assert!(ordinary(&view).1.is_empty());
    for control in [10010, 20, 21, 22, 23, 24, u64::MAX] {
        assert_eq!(view.node_for_control(ControlId(control)).unwrap(), None);
    }
}

#[test]
fn browser_capabilities_do_not_invent_native_device_actions() {
    let view = DevicesView::new(ScreenInstanceId(442), 960, 720).unwrap();
    let sources = [
        BrowserInputSource {
            id: "keyboard",
            kind: BrowserInputKind::Keyboard,
            label: "KEYBOARD",
            detail: "SHARED BROWSER KEYBOARD",
            selectable: true,
        },
        BrowserInputSource {
            id: "hid",
            kind: BrowserInputKind::Hid,
            label: "HID",
            detail: "USER GESTURE REQUIRED",
            selectable: false,
        },
    ];
    for can_assign in [false, true] {
        view.update_browser(BrowserDevicesFrame {
            sources: &sources,
            can_assign,
            can_refresh: false,
            player: Some(PlayerId(8)),
            selected: Some(0),
            first: 0,
            pending: false,
            error: None,
            hovered: None,
            armed: None,
        })
        .unwrap();
        ordinary(&view);
        assert_eq!(
            view.node_for_control(ControlId(10000)).unwrap().is_some(),
            can_assign
        );
        assert_eq!(
            view.node_for_control(ControlId(20)).unwrap().is_some(),
            can_assign
        );
        assert!(view.node_for_control(ControlId(21)).unwrap().is_some());
        for control in [10001, 22, 23, 24] {
            assert_eq!(view.node_for_control(ControlId(control)).unwrap(), None);
        }
    }
}

#[test]
fn moving_device_row_keeps_fixed_parent_clip_and_reuses_geometry() {
    let owner = ScreenInstanceId(443);
    let view = DevicesView::new(owner, 960, 720).unwrap();
    native(&view, &catalog(2), 0, false);
    ordinary(&view);
    let node = view.node_for_control(ControlId(10000)).unwrap().unwrap();
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
    let original_hits = hit_geometry(&hits);
    for offset in [50.0, 50.5, 50.0] {
        let pose = UiTransform::new([offset, 0.0], [1.0, 1.0], 1.0).unwrap();
        scene
            .set_component_transforms(&[(component, pose)])
            .unwrap();
        assert_eq!(
            view.hit_components(&scene, owner, (80.0, 125.0)),
            Some(ControlId(10000))
        );
        assert_eq!(view.hit_components(&scene, owner, (25.0, 125.0)), None);
        assert_eq!(
            view.hit_components(&scene, owner, (929.999, 125.0)),
            Some(ControlId(10000))
        );
        assert_eq!(view.hit_components(&scene, owner, (930.0, 125.0)), None);
        assert_eq!(
            view.hit_components(&scene, ScreenInstanceId(999), (80.0, 125.0)),
            None
        );
        view.compose_components(&mut scene, &mut hits, owner, &[node])
            .unwrap();
        assert!(Arc::ptr_eq(&identity, scene.geometry_stamp().0));
        assert_eq!(revision, scene.geometry_stamp().1);
        assert_eq!(hit_geometry(&hits), original_hits);
        assert_eq!(scene.component_transform(component), Some(pose));
    }
    scene
        .set_component_transforms(&[(
            component,
            UiTransform::new([50.0, 0.0], [1.0, 1.0], 0.0).unwrap(),
        )])
        .unwrap();
    assert_eq!(view.hit_components(&scene, owner, (80.0, 125.0)), None);
    assert_eq!(view.hit((25.0, 125.0)), Some(ControlId(10000)));
}

#[test]
fn malformed_device_component_targets_refuse_without_mutation() {
    let owner = ScreenInstanceId(444);
    let view = DevicesView::new(owner, 960, 720).unwrap();
    native(&view, &catalog(2), 0, false);
    ordinary(&view);
    let node = view.node_for_control(ControlId(21)).unwrap().unwrap();
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
    let original_hits = hit_geometry(&hits);
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
        assert_eq!(hit_geometry(&hits), original_hits);
        assert_eq!(scene.component_transform(component), Some(pose));
        assert_eq!(
            view.hit_components(&scene, owner, (235.0, 625.0)),
            Some(ControlId(21))
        );
    }
}
