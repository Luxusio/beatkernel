//! Deferred native retained comparison pages and metadata identity isolation.
use super::*;
use crate::{
    local_players::PlayerId,
    result_archive::{ArchivedResult, ArchivedScore},
    play_result::{PlayResultScope, PlayResultOutcome},
    timing::TimingRecord,
    competition::OpponentKind,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
};
fn snapshot(label: &str) -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: OpponentKind::Own,
            label: label.into(),
            hits: 4,
            misses: 1,
            combo: 2,
            max_combo: 4,
            recorded_until: None,
        }],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Stopped,
            progress: None,
        }),
    }
}
fn preview(comparison: Option<Option<CompetitionSnapshot>>) -> RecordPreview {
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
        historical_score: Some(Arc::new(ArchivedScore {
            hits: 5,
            misses: 0,
            combo: 0,
            max_combo: 0,
            grades: (0..5).map(|grade| (grade, 1)).collect(),
            timing: TimingRecord::default(),
        })),
        historical_comparison: comparison.map(Arc::new),
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
fn label(scene: &Scene, text: &str) {
    let expected: Vec<_> = text.chars().map(crate::font::glyph_uv).collect();
    let actual: Vec<_> = scene.rectangles().iter().map(|rect| rect.uv).collect();
    assert!(
        actual
            .windows(expected.len())
            .any(|window| window == expected.as_slice()),
        "missing comparison caption {text}"
    );
}
#[test]
fn retained_saved_and_peer_pages_use_common_geometry_and_only_current_detail_controls() {
    let preview = preview(Some(Some(snapshot("saved.bkr"))));
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(931), 960, 720).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    for page in [2, 3] {
        view.update(frame(&directory, &catalog, &preview, page))
            .unwrap();
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        label(
            &scene,
            if page == 2 {
                "SAVED REPLAY OPERATION PREFIX"
            } else {
                "PEER-REPORTED NOT FINAL RANKING"
            },
        );
        label(
            &scene,
            if page == 2 {
                "STORED COMPARISONS PAGE 1 / 2"
            } else {
                "STORED COMPARISONS PAGE 2 / 2"
            },
        );
        assert!(hits.iter().any(|row| row.0 == ControlId(66)));
        assert!(hits.iter().any(|row| row.0 == ControlId(67)));
        assert_eq!(hits.iter().any(|row| row.0 == ControlId(68)), page == 2);
        assert!(!hits.iter().any(|row| row.0.0 >= 50000));
    }
}
#[test]
fn stable_comparison_arc_reuses_owned_snapshot_on_page_and_hover_changes_but_replacement_invalidates_cache()
 {
    let mut preview = preview(Some(Some(snapshot("saved.bkr"))));
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(932), 960, 720).unwrap();
    let mut catalog_frame = frame(&directory, &catalog, &preview, 0);
    catalog_frame.details = false;
    view.update(catalog_frame).unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    let paints = view.nodes.paints();
    view.update(frame(&directory, &catalog, &preview, 2))
        .unwrap();
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    let pointer = view
        .detail_cache
        .borrow()
        .as_ref()
        .unwrap()
        .presentation
        .comparison()
        .unwrap()
        .as_ref()
        .unwrap()
        .ghosts
        .as_ptr();
    for page in [3, 2] {
        let mut next = frame(&directory, &catalog, &preview, page);
        next.hovered = Some(ControlId(66));
        next.armed = Some(ControlId(66));
        view.update(next).unwrap();
        assert_eq!(view.nodes.paints(), paints);
        scene.clear();
        hits.clear();
        view.compose(&mut scene, &mut hits).unwrap();
        assert_eq!(
            view.detail_cache
                .borrow()
                .as_ref()
                .unwrap()
                .presentation
                .comparison()
                .unwrap()
                .as_ref()
                .unwrap()
                .ghosts
                .as_ptr(),
            pointer
        );
    }
    let mut same = frame(&directory, &catalog, &preview, 2);
    same.hovered = Some(ControlId(66));
    same.armed = Some(ControlId(66));
    view.update(same).unwrap();
    assert!(!view.dirty());
    let same_value = preview
        .historical_comparison
        .as_ref()
        .unwrap()
        .as_ref()
        .clone();
    preview.historical_comparison = Some(Arc::new(same_value));
    let mut replaced_identity = frame(&directory, &catalog, &preview, 2);
    replaced_identity.hovered = Some(ControlId(66));
    replaced_identity.armed = Some(ControlId(66));
    view.update(replaced_identity).unwrap();
    assert!(view.dirty());
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    preview.historical_comparison = Some(Arc::new(Some(snapshot("replacement.bkr"))));
    view.update(frame(&directory, &catalog, &preview, 2))
        .unwrap();
    assert!(view.dirty());
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    label(&scene, "replacement.bkr");
}
#[test]
fn known_empty_and_empty_snapshot_remain_distinct_and_invalid_removed_metadata_page_is_atomic() {
    let mut preview = preview(Some(None));
    let catalog = RecordCatalog {
        entries: vec![preview.path.clone()],
        truncated: false,
    };
    let directory = LineEditor::new("records", 4096).unwrap();
    let view = RecordsView::new(ScreenInstanceId(933), 960, 720).unwrap();
    view.update(frame(&directory, &catalog, &preview, 2))
        .unwrap();
    let mut scene = Scene::new(960, 720);
    let mut hits = Vec::new();
    view.compose(&mut scene, &mut hits).unwrap();
    label(&scene, "NO SELECTED COMPARISONS");
    preview.historical_comparison = Some(Arc::new(Some(CompetitionSnapshot {
        ghosts: vec![],
        network: None,
    })));
    view.update(frame(&directory, &catalog, &preview, 2))
        .unwrap();
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    label(&scene, "STORED COMPARISON SNAPSHOT EMPTY");
    let before: Vec<_> = scene
        .rectangles()
        .iter()
        .map(|rect| (rect.bounds, rect.uv, rect.color))
        .collect();
    preview.historical_comparison = None;
    assert!(
        view.update(frame(&directory, &catalog, &preview, 2))
            .is_err()
    );
    assert!(!view.dirty());
    scene.clear();
    hits.clear();
    view.compose(&mut scene, &mut hits).unwrap();
    assert_eq!(
        scene
            .rectangles()
            .iter()
            .map(|rect| (rect.bounds, rect.uv, rect.color))
            .collect::<Vec<_>>(),
        before
    );
}
