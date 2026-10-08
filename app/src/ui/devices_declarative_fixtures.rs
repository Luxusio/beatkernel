//! Actual retained Devices declaration: native/browser parity and shared layout publication.
use super::super::layout::{LayoutChange, LayoutUpdate, NodeId};
use super::*;
use crate::device_catalog::{DeviceChoice, DeviceRequest};

fn catalog(count: usize) -> DeviceCatalog {
    DeviceCatalog::new(
        DeviceRequest::LinuxKeyboard,
        (0..count)
            .map(|index| DeviceChoice {
                id: format!("/dev/input/event{index}"),
                label: format!("KEYBOARD {index}"),
                detail: format!("ORIGINAL METADATA {index}"),
                selectable: true,
            })
            .collect(),
    )
    .unwrap()
}
fn frame(catalog: &DeviceCatalog) -> DevicesFrame<'_> {
    DevicesFrame {
        catalog,
        player: Some(PlayerId(77)),
        selected: Some(0),
        first: 0,
        pending: false,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn browser<'a>(sources: &'a [BrowserInputSource<'a>]) -> BrowserDevicesFrame<'a> {
    BrowserDevicesFrame {
        sources,
        can_assign: true,
        can_refresh: true,
        player: Some(PlayerId(u32::MAX)),
        selected: (!sources.is_empty()).then_some(0),
        first: 0,
        pending: false,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn compose(view: &DevicesView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
    let mut scene = Scene::new(width, height);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    (scene, hits)
}
fn geometry(scene: &Scene) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    scene
        .rectangles()
        .iter()
        .map(|rect| (rect.bounds, rect.color, rect.uv))
        .collect()
}
fn regions(hits: &[(ControlId, Bounds)]) -> Vec<(u64, [i64; 4])> {
    hits.iter()
        .map(|(id, bounds)| (id.0, [bounds.x, bounds.y, bounds.width, bounds.height]))
        .collect()
}
fn leaf(view: &DevicesView, component: Component) -> NodeId {
    view.layout
        .leaves()
        .iter()
        .find(|node| node.component == component)
        .unwrap()
        .id
}

#[test]
fn native_device_rows_preserve_original_controls_identity_and_bounded_page() {
    let catalog = catalog(MAX_DEVICES);
    let view = DevicesView::new(ScreenInstanceId(901), 960, 720).unwrap();
    let mut page = frame(&catalog);
    page.first = 1014;
    page.selected = Some(1023);
    view.update(page).unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(regions(&hits)[0], (11014, [24, 120, 906, 34]));
    assert_eq!(regions(&hits)[9], (11023, [24, 471, 906, 34]));
    assert!(hits.iter().any(|(id, _)| *id == ControlId(20)));
    assert_eq!(view.id(), ScreenInstanceId(901));
    assert_eq!(view.hit((30.0, 125.0)), Some(ControlId(11014)));
    assert_eq!(
        view.hit((930.0, 125.0)),
        None,
        "right row boundary is exclusive"
    );
    let paints = view.nodes.paints();
    let ids = view.nodes.identities();
    let mut same = frame(&catalog);
    same.first = 1014;
    same.selected = Some(1023);
    view.update(same).unwrap();
    let (again, _) = compose(&view, 960, 720);
    assert_eq!(geometry(&again), geometry(&scene));
    assert_eq!(view.nodes.paints(), paints);
    assert_eq!(view.nodes.identities(), ids);
}

#[test]
fn empty_and_pending_native_catalog_keep_back_but_never_invent_selected_device() {
    let catalog = catalog(0);
    let view = DevicesView::new(ScreenInstanceId(902), 960, 720).unwrap();
    let mut empty = frame(&catalog);
    empty.selected = None;
    view.update(empty).unwrap();
    let (_, hits) = compose(&view, 960, 720);
    assert!(!hits.iter().any(|(id, _)| id.0 == 20 || id.0 >= 10000));
    assert!(hits.iter().any(|(id, _)| *id == ControlId(21)));
    let mut pending = frame(&catalog);
    pending.selected = None;
    pending.pending = true;
    view.update(pending).unwrap();
    assert!(compose(&view, 960, 720).1.is_empty());
    assert_eq!(view.hit((220.0, 625.0)), None);
}

#[test]
fn browser_kinds_literal_sources_and_capabilities_use_the_same_retained_view() {
    let sources = [
        BrowserInputSource {
            id: "hid:9007199254740993",
            kind: BrowserInputKind::Hid,
            label: "CONTROLLER",
            detail: "permission-owned source",
            selectable: true,
        },
        BrowserInputSource {
            id: "touch:17",
            kind: BrowserInputKind::Touch,
            label: "TOUCH",
            detail: "original source",
            selectable: false,
        },
    ];
    let view = DevicesView::new(ScreenInstanceId(903), 960, 720).unwrap();
    view.update_browser(browser(&sources)).unwrap();
    let (original, hits) = compose(&view, 960, 720);
    assert!(hits.iter().any(|(id, _)| *id == ControlId(10000)));
    assert!(!hits.iter().any(|(id, _)| *id == ControlId(10001)));
    let ids = view.nodes.identities();
    let paints = view.nodes.paints();
    view.update_browser(browser(&sources)).unwrap();
    assert_eq!(view.nodes.paints(), paints);
    let mut disabled = browser(&sources);
    disabled.selected = Some(1);
    disabled.can_refresh = false;
    view.update_browser(disabled).unwrap();
    let (_, hits) = compose(&view, 960, 720);
    assert!(!hits.iter().any(|(id, _)| [20, 22].contains(&id.0)));
    let mut unsupported = browser(&sources);
    unsupported.can_assign = false;
    unsupported.can_refresh = false;
    view.update_browser(unsupported).unwrap();
    let (_, hits) = compose(&view, 960, 720);
    assert!(!hits
        .iter()
        .any(|(id, _)| id.0 >= 10000 || [20, 22].contains(&id.0)));
    assert!(hits.iter().any(|(id, _)| *id == ControlId(21)));
    let native = catalog(2);
    view.update(frame(&native)).unwrap();
    assert_ne!(geometry(&compose(&view, 960, 720).0), geometry(&original));
    assert_eq!(
        view.nodes.identities(),
        ids,
        "metadata mode does not replace mounted view nodes"
    );
}

#[test]
fn resized_devices_share_painted_clips_and_hit_admission_without_remounting() {
    let catalog = catalog(20);
    let mut view = DevicesView::new(ScreenInstanceId(904), 960, 720).unwrap();
    view.update(frame(&catalog)).unwrap();
    let identities = view.nodes.identities();
    for (width, height) in [(1200, 800), (480, 360)] {
        view.resize(width, height).unwrap();
        let (scene, hits) = compose(&view, width, height);
        assert_eq!(view.nodes.identities(), identities);
        assert!(scene.rectangles().iter().all(|rect| rect.bounds[0] >= 0.0
            && rect.bounds[1] >= 0.0
            && rect.bounds[0] + rect.bounds[2] <= width as f32
            && rect.bounds[1] + rect.bounds[3] <= height as f32));
        for (id, bounds) in &hits {
            assert!(
                bounds.x >= 0
                    && bounds.y >= 0
                    && bounds.x + bounds.width <= width as i64
                    && bounds.y + bounds.height <= height as i64
            );
            assert_eq!(
                view.hit((bounds.x as f64 + 1.0, bounds.y as f64 + 1.0)),
                Some(*id)
            );
        }
        assert_eq!(view.hit((width as f64, height as f64)), None);
        let paints = view.nodes.paints();
        assert!(!view.resize(width, height).unwrap());
        assert_eq!(view.nodes.paints(), paints);
    }
}

#[test]
fn child_size_reflows_later_rows_and_failed_layout_edit_preserves_published_state() {
    let catalog = catalog(10);
    let mut view = DevicesView::new(ScreenInstanceId(905), 960, 720).unwrap();
    view.update(frame(&catalog)).unwrap();
    compose(&view, 960, 720);
    let first = leaf(&view, Component::Row(0));
    let ids = view.nodes.identities();
    assert!(view
        .update_layout(&[LayoutUpdate {
            id: first,
            change: LayoutChange::Size([906, 30])
        }])
        .unwrap());
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(regions(&hits)[0], (10000, [24, 120, 906, 30]));
    assert_eq!(regions(&hits)[1], (10001, [24, 155, 906, 34]));
    assert_eq!(view.hit((30.0, 156.0)), Some(ControlId(10001)));
    assert_eq!(view.nodes.identities(), ids);
    let paints = view.nodes.paints();
    let revision = view.layout.revision();
    for updates in [
        vec![LayoutUpdate {
            id: first,
            change: LayoutChange::Size([-1, 30]),
        }],
        vec![
            LayoutUpdate {
                id: first,
                change: LayoutChange::Size([906, 34]),
            },
            LayoutUpdate {
                id: NodeId(usize::MAX),
                change: LayoutChange::Origin([0, 0]),
            },
        ],
    ] {
        assert!(view.update_layout(&updates).is_err());
        assert_eq!(view.layout.revision(), revision);
        assert_eq!(view.nodes.paints(), paints);
        assert_eq!(view.nodes.identities(), ids);
        assert_eq!(geometry(&compose(&view, 960, 720).0), geometry(&scene));
    }
}

#[test]
fn metadata_refusal_and_zero_extent_never_publish_partial_hits_or_lose_original_view() {
    let catalog = catalog(3);
    let mut view = DevicesView::new(ScreenInstanceId(906), 960, 720).unwrap();
    view.update(frame(&catalog)).unwrap();
    let (before, hits) = compose(&view, 960, 720);
    let paints = view.nodes.paints();
    let identities = view.nodes.identities();
    let mut bad = frame(&catalog);
    bad.selected = Some(3);
    assert!(view.update(bad).is_err());
    assert_eq!(view.nodes.paints(), paints);
    assert_eq!(geometry(&compose(&view, 960, 720).0), geometry(&before));
    assert!(view.resize(0, 720).unwrap());
    let (hidden, hidden_hits) = compose(&view, 0, 720);
    assert!(hidden.rectangles().is_empty());
    assert!(hidden_hits.is_empty());
    assert_eq!(view.hit((30.0, 125.0)), None);
    assert_eq!(view.nodes.identities(), identities);
    assert!(view.resize(960, 720).unwrap());
    let (restored, restored_hits) = compose(&view, 960, 720);
    assert_eq!(geometry(&restored), geometry(&before));
    assert_eq!(regions(&restored_hits), regions(&hits));
    assert_eq!(view.id(), ScreenInstanceId(906));
    assert_eq!(view.nodes.identities(), identities);
}

#[test]
fn row_clip_is_shared_by_actual_paint_and_hit_without_reflowing_sibling() {
    let catalog = catalog(2);
    let mut view = DevicesView::new(ScreenInstanceId(907), 960, 720).unwrap();
    view.update(frame(&catalog)).unwrap();
    let row = leaf(&view, Component::Row(0));
    view.update_layout(&[LayoutUpdate {
        id: row,
        change: LayoutChange::Clip(Some(Bounds {
            x: 0,
            y: 0,
            width: 50,
            height: 10,
        })),
    }])
    .unwrap();
    let (scene, hits) = compose(&view, 960, 720);
    assert_eq!(regions(&hits)[0], (10000, [24, 120, 50, 10]));
    assert_eq!(regions(&hits)[1], (10001, [24, 159, 906, 34]));
    assert_eq!(view.hit((73.999, 129.999)), Some(ControlId(10000)));
    assert_eq!(view.hit((74.0, 125.0)), None);
    assert!(scene
        .rectangles()
        .iter()
        .filter(|rect| rect.bounds[1] >= 120.0 && rect.bounds[1] < 130.0)
        .all(|rect| rect.bounds[0] >= 24.0
            && rect.bounds[0] + rect.bounds[2] <= 74.0
            && rect.bounds[1] + rect.bounds[3] <= 130.0));
}
