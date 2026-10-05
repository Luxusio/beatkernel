//! Deferred retained historical subview and catalog-hit isolation.
use super::*;
use crate::{
    competition::ScoreSummary,
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::CompletedPlayResult,
    result_archive::{ArchivedResult, ArchivedScore},
};
fn preview(scored: bool) -> RecordPreview {
    let gauge = BmsGauge::default();
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let score = ScoreSummary {
        hits: 2,
        combo: 2,
        max_combo: 2,
        grades: [(u32::MAX, 2)].into_iter().collect(),
        ..Default::default()
    };
    RecordPreview {
        path: "memory.bkr".into(),
        records: 1,
        recorded_until: Some(Timestamp::ZERO),
        start: Timestamp::ZERO,
        end: None,
        historical: Some((
            PlayerId(u32::MAX),
            ArchivedResult {
                scope: result.scope(),
                outcome: result.outcome(),
                gauge: result.gauge(),
            },
        )),
        historical_score: scored.then(|| Arc::new(ArchivedScore::from_summary(&score).unwrap())),
        archive_error: None,
        score: Default::default(),
    }
}
fn frame<'a>(
    directory: &'a LineEditor,
    catalog: &'a RecordCatalog,
    preview: Option<&'a RecordPreview>,
    details: bool,
) -> RecordsFrame<'a> {
    RecordsFrame {
        directory,
        directory_focused: false,
        catalog: Some(catalog),
        selected: Some(0),
        first: 0,
        preview,
        pending: false,
        details,
        opponents: 0,
        selected_opponents: [0; 2],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn label(scene: &Scene, text: &str) {
    let expected: Vec<_> = text.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| rectangle.uv)
        .collect();
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected.as_slice()),
        "missing label {text}"
    );
}
#[test]
fn details_uses_common_stored_geometry_and_exposes_only_back_instead_of_catalog_hits() {
    let preview = preview(true);
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(911), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, Some(&preview), true))
        .unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    assert_eq!(
        hits.iter().map(|row| row.0).collect::<Vec<_>>(),
        [ControlId(66)]
    );
    assert_eq!(
        hit(
            &frame(&directory, &catalog, Some(&preview), true),
            Some((305., 576.))
        ),
        Some(ControlId(66))
    );
    assert_eq!(
        hit(
            &frame(&directory, &catalog, Some(&preview), true),
            Some((30., 180.))
        ),
        None
    );
    label(&scene, "HISTORICAL PLAYER 4294967295");
    label(&scene, "STORED HITS 2 MISSES 0");
    label(&scene, "TIMING SUM 0 NS");
    assert!(scene.playfields().is_empty());
    view.update(frame(&directory, &catalog, Some(&preview), false))
        .unwrap();
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    assert!(hits.iter().any(|row| row.0 == ControlId(50000)));
    assert!(hits.iter().any(|row| row.0 == ControlId(66)));
}
#[test]
fn identical_detail_frames_retain_shared_grade_allocation_and_geometry_until_arc_identity_changes()
{
    let mut preview = preview(true);
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(912), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, Some(&preview), true))
        .unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let packet: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect();
    let original = preview.historical_score.as_ref().unwrap().clone();
    let grades = original.grades.as_ptr();
    let paints = view.nodes.paints();
    for _ in 0..3 {
        view.update(frame(&directory, &catalog, Some(&preview), true))
            .unwrap();
        assert!(!view.dirty());
        assert_eq!(view.nodes.paints(), paints);
        let cache = view.detail_cache.borrow();
        let retained = cache.as_ref().unwrap().score.as_ref().unwrap();
        assert!(Arc::ptr_eq(retained, &original));
        assert_eq!(retained.grades.as_ptr(), grades);
        drop(cache);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            scene
                .rectangles()
                .iter()
                .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
                .collect::<Vec<_>>(),
            packet
        );
    }
    preview.historical_score = Some(Arc::new(original.as_ref().clone()));
    view.update(frame(&directory, &catalog, Some(&preview), true))
        .unwrap();
    assert!(view.dirty());
    assert!(Arc::ptr_eq(
        view.detail_cache
            .borrow()
            .as_ref()
            .unwrap()
            .score
            .as_ref()
            .unwrap(),
        preview.historical_score.as_ref().unwrap()
    ));
}
#[test]
fn malformed_pending_or_stale_detail_frames_refuse_before_changing_current_mode_or_cached_value() {
    let preview = preview(true);
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(913), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, Some(&preview), true))
        .unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let original = preview.historical_score.as_ref().unwrap().clone();
    for field in 0..4 {
        let mut bad_preview = preview.clone();
        if field == 1 {
            bad_preview.historical = None;
        }
        if field == 2 {
            bad_preview.path = "foreign.bkr".into();
        }
        let mut bad = frame(
            &directory,
            &catalog,
            if field == 3 { None } else { Some(&bad_preview) },
            true,
        );
        if field == 0 {
            bad.pending = true;
        }
        assert!(view.update(bad).is_err());
        assert!(view.details.get());
        assert!(!view.dirty());
        assert!(Arc::ptr_eq(
            view.detail_cache
                .borrow()
                .as_ref()
                .unwrap()
                .score
                .as_ref()
                .unwrap(),
            &original
        ));
    }
}
#[test]
fn legacy_historical_details_explicitly_report_unavailable_score_and_invalid_score_is_atomic() {
    let mut preview = preview(false);
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(914), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, Some(&preview), true))
        .unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    label(&scene, "STORED SCORE UNAVAILABLE");
    assert!(view.detail_cache.borrow().as_ref().unwrap().score.is_none());
    let mut invalid = ArchivedScore::from_summary(&ScoreSummary::default()).unwrap();
    invalid.hits = 1;
    preview.historical_score = Some(Arc::new(invalid));
    assert!(
        view.update(frame(&directory, &catalog, Some(&preview), true))
            .is_err()
    );
    assert!(view.detail_cache.borrow().as_ref().unwrap().score.is_none());
    assert!(!view.dirty());
}

#[test]
fn detail_hover_and_armed_updates_repaint_only_cached_detail_packet_and_leave_catalog_nodes_untouched()
 {
    let preview = preview(true);
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(915), 960, 720).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.update(frame(&directory, &catalog, Some(&preview), false))
        .unwrap();
    view.compose(&mut scene, &mut hits).unwrap();
    assert!(hits.iter().any(|row| row.0 == ControlId(66)));
    let catalog_paints = view.nodes.paints();
    view.update(frame(&directory, &catalog, Some(&preview), true))
        .unwrap();
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    let normal: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect();
    assert_eq!(view.nodes.paints(), catalog_paints);
    let mut hover = frame(&directory, &catalog, Some(&preview), true);
    hover.hovered = Some(ControlId(66));
    hover.armed = Some(ControlId(66));
    view.update(hover).unwrap();
    assert!(view.dirty());
    assert_eq!(view.nodes.paints(), catalog_paints);
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    let active: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect();
    assert_ne!(active, normal);
    assert_eq!(
        hits.iter().map(|row| row.0).collect::<Vec<_>>(),
        [ControlId(66)]
    );
    let mut repeat = frame(&directory, &catalog, Some(&preview), true);
    repeat.hovered = Some(ControlId(66));
    repeat.armed = Some(ControlId(66));
    view.update(repeat).unwrap();
    assert!(!view.dirty());
    assert_eq!(view.nodes.paints(), catalog_paints);
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
            .collect::<Vec<_>>(),
        active
    );
    view.update(frame(&directory, &catalog, Some(&preview), false))
        .unwrap();
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    assert!(hits.iter().any(|row| row.0 == ControlId(50000)));
    assert!(hits.iter().any(|row| row.0 == ControlId(58)));
    assert!(hits.iter().any(|row| row.0 == ControlId(66)));
}
