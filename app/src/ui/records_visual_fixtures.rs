//! Same Records view with native/frozen read-only inputs and retained historical cache.
use super::*;
use crate::record_model::FrozenRecordPreview;

fn source_record() -> RecordPreview {
    crate::record_model::visual_fixtures::record()
}
fn native_frame<'a>(
    directory: &'a LineEditor,
    catalog: &'a RecordCatalog,
    preview: &'a RecordPreview,
) -> RecordsFrame<'a> {
    RecordsFrame {
        directory,
        directory_focused: false,
        catalog: Some(catalog),
        selected: Some(0),
        first: 0,
        preview: Some(preview),
        pending: false,
        details: false,
        grade_page: 0,
        opponents: 2,
        selected_opponents: [1, 1],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn visual_frame<'a>(
    directory: &'a LineEditor,
    catalog: &'a RecordCatalog,
    preview: &'a FrozenRecordPreview,
) -> VisualRecordsFrame<'a> {
    VisualRecordsFrame {
        directory,
        directory_focused: false,
        catalog: Some(catalog),
        selected: Some(0),
        first: 0,
        preview: Some(preview),
        pending: false,
        details: false,
        grade_page: 0,
        opponents: 2,
        selected_opponents: [1, 1],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn compose(view: &RecordsView, width: u32, height: u32) -> (Scene, Vec<(ControlId, Bounds)>) {
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
        .map(|(id, b)| (id.0, [b.x, b.y, b.width, b.height]))
        .collect()
}

#[test]
fn actual_native_and_frozen_records_have_identical_timing_class_paint_and_actions() {
    let record = source_record();
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    let catalog = RecordCatalog {
        entries: vec![record.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let native = RecordsView::new(ScreenInstanceId(1001), 960, 720).unwrap();
    let visual = RecordsView::new(ScreenInstanceId(1002), 960, 720).unwrap();
    native
        .update(native_frame(&directory, &catalog, &record))
        .unwrap();
    visual
        .update_visual(visual_frame(&directory, &catalog, &frozen))
        .unwrap();
    let (expected, expected_hits) = compose(&native, 960, 720);
    let (actual, actual_hits) = compose(&visual, 960, 720);
    assert_eq!(geometry(&actual), geometry(&expected));
    assert_eq!(regions(&actual_hits), regions(&expected_hits));
    for (_, bounds) in &actual_hits {
        let point = Some((bounds.x as f64 + 1.0, bounds.y as f64 + 1.0));
        assert_eq!(
            hit_visual(&visual_frame(&directory, &catalog, &frozen), point),
            hit(&native_frame(&directory, &catalog, &record), point)
        );
    }
    let identities = visual.nodes.identities();
    let paints = visual.nodes.paints();
    visual
        .update_visual(visual_frame(&directory, &catalog, &frozen))
        .unwrap();
    compose(&visual, 960, 720);
    assert_eq!(visual.nodes.identities(), identities);
    assert_eq!(visual.nodes.paints(), paints);
}

#[test]
fn native_and_visual_history_share_original_arc_cached_detail_geometry() {
    let record = source_record();
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    let catalog = RecordCatalog {
        entries: vec![record.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let native = RecordsView::new(ScreenInstanceId(1003), 960, 720).unwrap();
    let visual = RecordsView::new(ScreenInstanceId(1004), 960, 720).unwrap();
    let mut nf = native_frame(&directory, &catalog, &record);
    nf.details = true;
    native.update(nf).unwrap();
    let mut vf = visual_frame(&directory, &catalog, &frozen);
    vf.details = true;
    visual.update_visual(vf).unwrap();
    let (expected, expected_hits) = compose(&native, 960, 720);
    let (actual, actual_hits) = compose(&visual, 960, 720);
    assert_eq!(geometry(&actual), geometry(&expected));
    assert_eq!(regions(&actual_hits), regions(&expected_hits));
    let cache = visual.detail_cache.borrow();
    let cached = cache.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        cached.score.as_ref().unwrap(),
        frozen.historical_score.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        cached.comparison.as_ref().unwrap(),
        frozen.historical_comparison.as_ref().unwrap()
    ));
    drop(cache);
    let identities = visual.nodes.identities();
    let paints = visual.nodes.paints();
    let mut vf = visual_frame(&directory, &catalog, &frozen);
    vf.details = true;
    visual.update_visual(vf).unwrap();
    assert_eq!(geometry(&compose(&visual, 960, 720).0), geometry(&actual));
    assert_eq!(visual.nodes.paints(), paints);
    assert_eq!(visual.nodes.identities(), identities);
}

#[test]
fn malformed_selected_timing_class_and_history_refuse_before_any_visual_publication() {
    let record = source_record();
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    let catalog = RecordCatalog {
        entries: vec![record.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(1005), 960, 720).unwrap();
    view.update_visual(visual_frame(&directory, &catalog, &frozen))
        .unwrap();
    let (original, original_hits) = compose(&view, 960, 720);
    let paints = view.nodes.paints();
    let mut cases = Vec::new();
    let mut bad = frozen.clone();
    bad.path = "other.bkr".into();
    cases.push(bad);
    let mut bad = frozen.clone();
    bad.score.timing.exact += 1;
    cases.push(bad);
    let mut bad = frozen.clone();
    bad.bms_score.as_mut().unwrap().ex_score += 1;
    cases.push(bad);
    let mut bad = frozen.clone();
    bad.historical = None;
    cases.push(bad);
    for bad in cases {
        assert!(view
            .update_visual(visual_frame(&directory, &catalog, &bad))
            .is_err());
        assert_eq!(view.nodes.paints(), paints);
        let (scene, hits) = compose(&view, 960, 720);
        assert_eq!(geometry(&scene), geometry(&original));
        assert_eq!(regions(&hits), regions(&original_hits));
    }
    let mut invalid = visual_frame(&directory, &catalog, &frozen);
    invalid.selected = Some(1);
    assert!(view.update_visual(invalid).is_err());
    assert_eq!(view.nodes.paints(), paints);
}

#[test]
fn frozen_archive_error_keeps_valid_prefix_controls_and_uses_same_resize_lifecycle() {
    let mut record = source_record();
    record.historical = None;
    record.historical_score = None;
    record.historical_comparison = None;
    record.historical_bms_score = None;
    record.archive_error = Some("invalid sidecar association".into());
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    let catalog = RecordCatalog {
        entries: vec![record.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(1006), 960, 720).unwrap();
    view.update_visual(visual_frame(&directory, &catalog, &frozen))
        .unwrap();
    let identities = view.nodes.identities();
    let (original, hits) = compose(&view, 960, 720);
    assert!(hits.iter().any(|(id, _)| *id == ControlId(52)));
    assert!(!hits.iter().any(|(id, _)| *id == ControlId(66)));
    view.resize(480, 360).unwrap();
    let (small, small_hits) = compose(&view, 480, 360);
    assert!(small.rectangles().iter().all(|rect| rect.bounds[0] >= 0.0
        && rect.bounds[1] >= 0.0
        && rect.bounds[0] + rect.bounds[2] <= 480.0
        && rect.bounds[1] + rect.bounds[3] <= 360.0));
    for (id, bounds) in &small_hits {
        assert_eq!(
            view.hit((bounds.x as f64 + 1.0, bounds.y as f64 + 1.0)),
            Some(*id)
        );
    }
    view.resize(0, 720).unwrap();
    let (hidden, hidden_hits) = compose(&view, 0, 720);
    assert!(hidden.rectangles().is_empty());
    assert!(hidden_hits.is_empty());
    view.resize(960, 720).unwrap();
    assert_eq!(geometry(&compose(&view, 960, 720).0), geometry(&original));
    assert_eq!(view.nodes.identities(), identities);
    assert_eq!(frozen.score.hits, 1);
    assert_eq!(frozen.score.misses, 0);
}

#[test]
fn opaque_archived_grade_pages_have_native_visual_parity_and_atomic_last_page_refusal() {
    let mut record = source_record();
    record.historical_bms_score = None;
    record.historical_score = Some(Arc::new(crate::result_archive::ArchivedScore {
        hits: 17,
        misses: 0,
        combo: 0,
        max_combo: 0,
        grades: (0..17).map(|index| (u32::MAX - 16 + index, 1)).collect(),
        timing: crate::timing::TimingRecord::default(),
    }));
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    let catalog = RecordCatalog {
        entries: vec![record.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let native = RecordsView::new(ScreenInstanceId(1007), 960, 720).unwrap();
    let visual = RecordsView::new(ScreenInstanceId(1008), 960, 720).unwrap();
    for page in [0, 1] {
        let mut nf = native_frame(&directory, &catalog, &record);
        nf.details = true;
        nf.grade_page = page;
        native.update(nf).unwrap();
        let mut vf = visual_frame(&directory, &catalog, &frozen);
        vf.details = true;
        vf.grade_page = page;
        visual.update_visual(vf).unwrap();
        let (expected, eh) = compose(&native, 960, 720);
        let (actual, ah) = compose(&visual, 960, 720);
        assert_eq!(geometry(&actual), geometry(&expected));
        assert_eq!(regions(&ah), regions(&eh));
    }
    let (before, hits) = compose(&visual, 960, 720);
    let paints = visual.nodes.paints();
    let mut bad = visual_frame(&directory, &catalog, &frozen);
    bad.details = true;
    bad.grade_page = usize::MAX;
    assert!(visual.update_visual(bad).is_err());
    let (after, ah) = compose(&visual, 960, 720);
    assert_eq!(geometry(&after), geometry(&before));
    assert_eq!(regions(&ah), regions(&hits));
    assert_eq!(visual.nodes.paints(), paints);
}
