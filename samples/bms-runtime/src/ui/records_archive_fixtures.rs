//! Deferred actual retained Records geometry, with no GPU or filesystem.
use super::*;
use crate::{
    gauge::{BmsGauge, GaugeProfile},
    local_players::PlayerId,
    play_result::CompletedPlayResult,
    result_archive::ArchivedResult,
};
fn frame<'a>(
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
        opponents: 0,
        selected_opponents: [0; 2],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn preview(practice: bool) -> RecordPreview {
    let gauge =
        BmsGauge::new(GaugeProfile::new(27_123_456, 20_000_000, 0, 0, false, vec![]).unwrap());
    let end = practice.then_some(Timestamp::from_nanos(604_800_000_000_000));
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, end, &gauge);
    RecordPreview {
        path: "records/original.bkr".into(),
        records: 0,
        recorded_until: None,
        start: Timestamp::ZERO,
        end,
        score: Default::default(),
        historical: Some((
            PlayerId(u32::MAX),
            ArchivedResult {
                scope: result.scope(),
                outcome: result.outcome(),
                gauge: result.gauge(),
            },
        )),
        historical_score: None,
        archive_error: None,
    }
}
fn label(scene: &Scene, text: &str) {
    let expected = text.chars().map(crate::font::glyph_uv).collect::<Vec<_>>();
    let actual = scene
        .rectangles()
        .iter()
        .map(|rectangle| rectangle.uv)
        .collect::<Vec<_>>();
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected.as_slice()),
        "missing literal historical label {text}"
    );
}
#[test]
fn stored_historical_id_gauge_and_scope_are_distinct_from_empty_prefix_score() {
    for practice in [false, true] {
        let preview = preview(practice);
        let catalog = RecordCatalog {
            entries: vec![preview.path.clone()],
            truncated: false,
        };
        let directory = LineEditor::new("records", 4096).unwrap();
        let view = RecordsView::new(ScreenInstanceId(72), 960, 720).unwrap();
        view.update(frame(&directory, &catalog, &preview)).unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        label(&scene, "HISTORICAL PLAYER 4294967295");
        label(&scene, "STORED GAUGE 27123456 UNITS");
        label(&scene, "STORED OUTCOME CLEARED");
        label(
            &scene,
            if practice {
                "STORED SCOPE PRACTICE SECTION"
            } else {
                "STORED SCOPE FULL SONG"
            },
        );
        assert_eq!(preview.score.hits, 0);
        assert_eq!(preview.score.misses, 0);
        assert_eq!(preview.recorded_until, None);
    }
}
#[test]
fn end_and_archive_refusal_changes_repaint_cached_geometry_while_identical_update_reuses_packets() {
    let mut preview = preview(false);
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(73), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, &preview)).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let before = view.nodes.paints();
    let packet = scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv))
        .collect::<Vec<_>>();
    view.update(frame(&directory, &catalog, &preview)).unwrap();
    assert_eq!(view.nodes.paints(), before);
    assert!(!view.dirty());
    scene.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|rectangle| (rectangle.bounds, rectangle.uv))
            .collect::<Vec<_>>(),
        packet
    );
    preview.end = Some(Timestamp::from_nanos(72_000_000_000_000));
    view.update(frame(&directory, &catalog, &preview)).unwrap();
    assert_ne!(view.nodes.paints(), before);
    scene.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    label(&scene, "END 72000000000000 NS");
    preview.historical = None;
    preview.archive_error = Some("malformed later archive row".into());
    view.update(frame(&directory, &catalog, &preview)).unwrap();
    scene.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    label(&scene, "HISTORICAL ARCHIVE UNAVAILABLE");
    assert_eq!(preview.score.misses, 0);
    let after = view.nodes.paints();
    preview.archive_error = Some("different private diagnostic".into());
    view.update(frame(&directory, &catalog, &preview)).unwrap();
    assert_eq!(view.nodes.paints(), after);
}
