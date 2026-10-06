//! Deferred native retained grade leaf staging and control isolation.
use super::*;
use crate::{
    result_archive::{ArchivedResult, ArchivedScore},
    play_result::{PlayResultScope, PlayResultOutcome},
    local_players::PlayerId,
    timing::TimingRecord,
};
fn preview(count: Option<usize>) -> RecordPreview {
    let score = count.map(|count| ArchivedScore {
        hits: count as u64,
        misses: 0,
        combo: 0,
        max_combo: 0,
        grades: (0..count)
            .map(|index| {
                (
                    if index + 1 == count {
                        u32::MAX
                    } else {
                        index as u32
                    },
                    1,
                )
            })
            .collect(),
        timing: TimingRecord::default(),
    });
    RecordPreview {
        path: "memory.bkr".into(),
        records: 0,
        recorded_until: None,
        start: Timestamp::ZERO,
        end: None,
        historical: Some((
            PlayerId(u32::MAX),
            ArchivedResult {
                scope: PlayResultScope::FullSong,
                outcome: PlayResultOutcome::BelowClearThreshold,
                gauge: *crate::gauge::BmsGauge::default().snapshot(),
            },
        )),
        historical_score: score.map(Arc::new),
        historical_comparison: None,
        archive_error: None,
        score: Default::default(),
    }
}
fn frame<'a>(
    directory: &'a LineEditor,
    catalog: &'a RecordCatalog,
    preview: &'a RecordPreview,
    page: usize,
) -> RecordsFrame<'a> {
    RecordsFrame {
        directory,
        directory_focused: false,
        catalog: Some(catalog),
        selected: Some(0),
        first: 0,
        preview: Some(preview),
        pending: false,
        details: true,
        grade_page: page,
        opponents: 0,
        selected_opponents: [0; 2],
        message: None,
        error: None,
        hovered: None,
        armed: None,
    }
}
fn label(scene: &Scene, value: &str) {
    let expected: Vec<_> = value.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| rectangle.uv)
        .collect();
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected.as_slice())
    );
}
#[test]
fn first_middle_and_last_grade_pages_render_exact_rows_and_only_enabled_detail_controls() {
    let preview = preview(Some(9));
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(921), 960, 720).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    for (page, expected) in [(0, vec![66, 68]), (1, vec![66, 67, 68]), (2, vec![66, 67])] {
        view.update(frame(&directory, &catalog, &preview, page))
            .unwrap();
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        let mut ids: Vec<_> = hits.iter().map(|row| row.0.0).collect();
        ids.sort();
        assert_eq!(ids, expected);
        assert_eq!(
            hit(
                &frame(&directory, &catalog, &preview, page),
                Some((755., 621.))
            ),
            Some(ControlId(66))
        );
        assert_eq!(
            hit(
                &frame(&directory, &catalog, &preview, page),
                Some((431., 576.))
            ),
            (page > 0).then_some(ControlId(67))
        );
        assert_eq!(
            hit(
                &frame(&directory, &catalog, &preview, page),
                Some((581., 576.))
            ),
            (page < 2).then_some(ControlId(68))
        );
        let grade = if page == 2 { u32::MAX } else { page as u32 * 4 };
        label(&scene, &format!("STORED GRADE {grade} COUNT 1"));
        assert!(!hits.iter().any(|row| row.0.0 >= 50000));
    }
}
#[test]
fn page_changes_reuse_common_grade_allocation_and_hover_does_not_repaint_hidden_catalog() {
    let preview = preview(Some(9));
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(922), 960, 720).unwrap();
    let mut catalog_frame = frame(&directory, &catalog, &preview, 0);
    catalog_frame.details = false;
    view.update(catalog_frame).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let paints = view.nodes.paints();
    view.update(frame(&directory, &catalog, &preview, 0))
        .unwrap();
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    let grades = view
        .detail_cache
        .borrow()
        .as_ref()
        .unwrap()
        .presentation
        .score()
        .unwrap()
        .grades
        .as_ptr();
    view.update(frame(&directory, &catalog, &preview, 1))
        .unwrap();
    assert_eq!(
        view.detail_cache
            .borrow()
            .as_ref()
            .unwrap()
            .presentation
            .score()
            .unwrap()
            .grades
            .as_ptr(),
        grades
    );
    assert_eq!(view.nodes.paints(), paints);
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    let mut active = frame(&directory, &catalog, &preview, 1);
    active.hovered = Some(ControlId(68));
    active.armed = Some(ControlId(68));
    view.update(active).unwrap();
    assert!(view.dirty());
    assert_eq!(view.nodes.paints(), paints);
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    let mut same = frame(&directory, &catalog, &preview, 1);
    same.hovered = Some(ControlId(68));
    same.armed = Some(ControlId(68));
    view.update(same).unwrap();
    assert!(!view.dirty());
    assert_eq!(
        view.detail_cache
            .borrow()
            .as_ref()
            .unwrap()
            .presentation
            .score()
            .unwrap()
            .grades
            .as_ptr(),
        grades
    );
}
#[test]
fn invalid_pages_pending_and_stale_frames_cannot_half_publish_a_prepared_leaf() {
    let preview = preview(Some(9));
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(923), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, &preview, 1))
        .unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let before: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
        .collect();
    for invalid in 0..4 {
        let mut stale = preview.clone();
        if invalid == 3 {
            stale.path = "foreign.bkr".into();
        }
        let mut bad = frame(
            &directory,
            &catalog,
            &stale,
            if invalid == 0 {
                3
            } else if invalid == 1 {
                usize::MAX
            } else {
                2
            },
        );
        if invalid == 2 {
            bad.pending = true;
        }
        assert!(view.update(bad).is_err());
        assert!(!view.dirty());
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            scene
                .rectangles()
                .iter()
                .map(|rectangle| (rectangle.bounds, rectangle.uv, rectangle.color))
                .collect::<Vec<_>>(),
            before
        );
    }
}
#[test]
fn unavailable_empty_and_one_page_metadata_have_back_only_and_distinct_grade_captions() {
    for count in [None, Some(0), Some(4)] {
        let preview = preview(count);
        let catalog = RecordCatalog {
            entries: vec![preview.path.clone()],
            truncated: false,
        };
        let directory = LineEditor::new("records", 4096).unwrap();
        let view = RecordsView::new(ScreenInstanceId(924), 960, 720).unwrap();
        view.update(frame(&directory, &catalog, &preview, 0))
            .unwrap();
        let mut scene = Scene::new(960, 720);
        let mut hits = Vec::new();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            hits.iter().map(|row| row.0).collect::<Vec<_>>(),
            [ControlId(66)]
        );
        if count.is_none() {
            label(&scene, "STORED GRADES UNAVAILABLE");
        }
        if count == Some(0) {
            label(&scene, "STORED GRADES EMPTY");
        }
    }
}
