//! Public Records presentation contracts, without a GPU, filesystem or IO owner.
#![cfg(feature = "graphics")]

use std::sync::Arc;

use beatkernel::time::Timestamp;
use beatkernel_bms_runtime::{
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::{PlayResultOutcome, PlayResultScope},
    record_model::{RecordCatalog, RecordPreview},
    result_archive::{ArchivedResult, ArchivedScore},
    scene::{Scene, UiComponentKey, UiTransform},
    screen_lifecycle::ScreenInstanceId,
    timing::TimingRecord,
    ui::{
        interaction::{Bounds, ControlId},
        layout::NodeId,
        records::{RecordsFrame, RecordsView},
        text_input::LineEditor,
    },
};

fn catalog(count: usize) -> RecordCatalog {
    RecordCatalog {
        entries: (0..count)
            .map(|i| format!("records/{i}.bkr").into())
            .collect(),
        truncated: false,
    }
}

fn frame<'a>(directory: &'a LineEditor, catalog: &'a RecordCatalog) -> RecordsFrame<'a> {
    RecordsFrame {
        directory,
        directory_focused: true,
        catalog: Some(catalog),
        selected: (!catalog.entries.is_empty()).then_some(0),
        first: 0,
        preview: None,
        pending: false,
        details: false,
        grade_page: 0,
        opponents: 0,
        selected_opponents: [0; 2],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}

fn historical_preview() -> RecordPreview {
    RecordPreview {
        path: "records/0.bkr".into(),
        records: 0,
        recorded_until: None,
        start: Timestamp::ZERO,
        end: None,
        historical: Some((
            PlayerId(1),
            ArchivedResult {
                scope: PlayResultScope::FullSong,
                outcome: PlayResultOutcome::BelowClearThreshold,
                gauge: *BmsGauge::default().snapshot(),
            },
        )),
        historical_score: Some(Arc::new(ArchivedScore {
            hits: 9,
            misses: 0,
            combo: 0,
            max_combo: 0,
            grades: (0..9).map(|grade| (grade, 1)).collect(),
            timing: TimingRecord::default(),
        })),
        bms_score: None,
        historical_bms_score: None,
        historical_comparison: None,
        archive_error: None,
        score: Default::default(),
    }
}

fn center(bounds: Bounds) -> (f64, f64) {
    (
        (bounds.x + bounds.width / 2) as f64,
        (bounds.y + bounds.height / 2) as f64,
    )
}

fn hit_values(hits: &[(ControlId, Bounds)]) -> Vec<(ControlId, i64, i64, i64, i64)> {
    hits.iter()
        .map(|(id, bounds)| (*id, bounds.x, bounds.y, bounds.width, bounds.height))
        .collect()
}

#[test]
fn chooser_motion_changes_hits_without_rebuilding_geometry_or_business_controls() {
    let owner = ScreenInstanceId(2001);
    let view = RecordsView::new(owner, 960, 720).unwrap();
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(12);
    view.update(frame(&directory, &catalog)).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let original_hits = hit_values(&hits);
    let control = ControlId(50);
    let node = view.node_for_control(control).unwrap().unwrap();
    let original = hits.iter().find(|(id, _)| *id == control).unwrap().1;
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    let id = scene
        .component_id(UiComponentKey {
            screen: owner,
            node,
        })
        .unwrap();
    let identity = scene.geometry_stamp().0.clone();
    let revision = scene.geometry_stamp().1;
    let point = (
        (original.x + original.width - 2) as f64,
        (original.y + original.height / 2) as f64,
    );
    // The fixed footer row clips vertical movement out of its 34px height.
    scene
        .set_component_transforms(&[(
            id,
            UiTransform::new([0.0, -100.0], [1.0, 1.0], 1.0).unwrap(),
        )])
        .unwrap();
    assert_ne!(
        view.hit_components(&scene, owner, (point.0, point.1 - 100.0)),
        Some(control)
    );
    // Move left within that row, avoiding later-painted neighboring buttons.
    for offset in [-20.0, -20.5, -20.0] {
        scene
            .set_component_transforms(&[(
                id,
                UiTransform::new([offset, 0.0], [1.0, 1.0], 1.0).unwrap(),
            )])
            .unwrap();
        view.compose_components(&mut scene, &mut hits, owner, &[node])
            .unwrap();
        assert_eq!(
            view.hit_components(&scene, owner, (point.0 + offset as f64, point.1)),
            Some(control)
        );
        assert_ne!(view.hit_components(&scene, owner, point), Some(control));
        assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
        assert_eq!(scene.geometry_stamp().1, revision);
        assert_eq!(
            scene.component_id(UiComponentKey {
                screen: owner,
                node
            }),
            Some(id)
        );
        assert_eq!(hit_values(&hits), original_hits);
    }
    assert_eq!(
        view.hit_components(&scene, ScreenInstanceId(2002), point),
        None
    );
    assert_eq!(
        view.hit_components(&scene, owner, (f64::NAN, point.1)),
        None
    );
}

#[test]
fn catalog_paging_reassociates_mounted_row_and_rejects_absent_actions() {
    let owner = ScreenInstanceId(2003);
    let view = RecordsView::new(owner, 960, 720).unwrap();
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(12);
    view.update(frame(&directory, &catalog)).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let first_row = view.node_for_control(ControlId(50000)).unwrap().unwrap();
    assert_eq!(view.node_for_control(ControlId(56)).unwrap(), None);
    assert!(view.node_for_control(ControlId(57)).unwrap().is_some());
    assert_eq!(view.node_for_control(ControlId(u64::MAX)).unwrap(), None);
    let mut next = frame(&directory, &catalog);
    next.first = 10;
    next.selected = Some(10);
    view.update(next).unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[first_row])
        .unwrap();
    assert_eq!(view.node_for_control(ControlId(50000)).unwrap(), None);
    assert_eq!(
        view.node_for_control(ControlId(50010)).unwrap(),
        Some(first_row)
    );
    assert_eq!(view.node_for_control(ControlId(57)).unwrap(), None);
    assert!(view.node_for_control(ControlId(56)).unwrap().is_some());
    assert_eq!(
        view.hit_components(&scene, owner, (30.0, 180.0)),
        Some(ControlId(50010))
    );
}

#[test]
fn malformed_component_requests_preserve_published_scene_and_hits() {
    let owner = ScreenInstanceId(2004);
    let view = RecordsView::new(owner, 960, 720).unwrap();
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(1);
    view.update(frame(&directory, &catalog)).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let node = view.node_for_control(ControlId(55)).unwrap().unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    let id = scene
        .component_id(UiComponentKey {
            screen: owner,
            node,
        })
        .unwrap();
    let identity = scene.geometry_stamp().0.clone();
    let revision = scene.geometry_stamp().1;
    let original_hits = hit_values(&hits);
    for (screen, nodes) in [
        (owner, vec![node, node]),
        (owner, vec![NodeId(usize::MAX)]),
        (ScreenInstanceId(0), vec![node]),
    ] {
        assert!(view
            .compose_components(&mut scene, &mut hits, screen, &nodes)
            .is_err());
        assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
        assert_eq!(scene.geometry_stamp().1, revision);
        assert_eq!(
            scene.component_id(UiComponentKey {
                screen: owner,
                node
            }),
            Some(id)
        );
        assert_eq!(hit_values(&hits), original_hits);
    }
}

#[test]
fn moving_catalog_row_remains_clipped_by_fixed_list_parent() {
    let owner = ScreenInstanceId(2005);
    let view = RecordsView::new(owner, 960, 720).unwrap();
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(10);
    view.update(frame(&directory, &catalog)).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let node = view.node_for_control(ControlId(50000)).unwrap().unwrap();
    view.compose_components(&mut scene, &mut hits, owner, &[node])
        .unwrap();
    let id = scene
        .component_id(UiComponentKey {
            screen: owner,
            node,
        })
        .unwrap();
    scene
        .set_component_transforms(&[(id, UiTransform::new([0.0, -20.0], [1.0, 1.0], 1.0).unwrap())])
        .unwrap();
    assert_eq!(
        view.hit_components(&scene, owner, (30.0, 170.0)),
        Some(ControlId(50000))
    );
    assert_ne!(
        view.hit_components(&scene, owner, (30.0, 169.999)),
        Some(ControlId(50000))
    );
    assert_ne!(
        view.hit_components(&scene, owner, (30.0, 178.0)),
        Some(ControlId(50000))
    );
}

#[test]
fn historical_details_motion_uses_visible_detail_controls_on_actual_grade_pages() {
    let owner = ScreenInstanceId(2006);
    let view = RecordsView::new(owner, 960, 720).unwrap();
    let directory = LineEditor::new("records", 4096).unwrap();
    let catalog = catalog(1);
    let preview = historical_preview();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    for page in [0, 1, 2] {
        let mut detail = frame(&directory, &catalog);
        detail.preview = Some(&preview);
        detail.details = true;
        detail.grade_page = page;
        view.update(detail).unwrap();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(view.node_for_control(ControlId(50)).unwrap(), None);
        assert_eq!(view.node_for_control(ControlId(50000)).unwrap(), None);
        let control = ControlId(66);
        let node = view.node_for_control(control).unwrap().unwrap();
        let bounds = hits.iter().find(|(id, _)| *id == control).unwrap().1;
        view.compose_components(&mut scene, &mut hits, owner, &[node])
            .unwrap();
        let id = scene
            .component_id(UiComponentKey {
                screen: owner,
                node,
            })
            .unwrap();
        let identity = scene.geometry_stamp().0.clone();
        let revision = scene.geometry_stamp().1;
        scene
            .set_component_transforms(&[(
                id,
                UiTransform::new([0.0, -80.0], [1.0, 1.0], 1.0).unwrap(),
            )])
            .unwrap();
        view.compose_components(&mut scene, &mut hits, owner, &[node])
            .unwrap();
        let point = center(bounds);
        assert_eq!(
            view.hit_components(&scene, owner, (point.0, point.1 - 80.0)),
            Some(control)
        );
        assert_ne!(view.hit_components(&scene, owner, point), Some(control));
        assert!(Arc::ptr_eq(scene.geometry_stamp().0, &identity));
        assert_eq!(scene.geometry_stamp().1, revision);
        assert_eq!(
            view.node_for_control(ControlId(67)).unwrap().is_some(),
            page > 0
        );
        assert_eq!(
            view.node_for_control(ControlId(68)).unwrap().is_some(),
            page < 2
        );
    }
}
