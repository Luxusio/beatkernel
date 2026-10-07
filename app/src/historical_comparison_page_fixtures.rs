//! Deferred stored comparison pages, distinct from completion/ranking authority.
use super::*;
use crate::{
    competition::OpponentKind,
    competition_presentation::{
        CompetitionSnapshot, GhostSnapshot, NetworkSnapshot, NetworkStatus,
    },
    result_archive::{ArchivedResult, ArchivedScore},
    timing::TimingRecord,
    multiplayer::Progress,
};
fn value() -> HistoricalRecordValue {
    (
        PlayerId(u32::MAX),
        ArchivedResult {
            scope: PlayResultScope::FullSong,
            outcome: PlayResultOutcome::BelowClearThreshold,
            gauge: *crate::gauge::BmsGauge::default().snapshot(),
        },
    )
}
fn score(count: usize) -> ArchivedScore {
    ArchivedScore {
        hits: count as u64,
        misses: 0,
        combo: 0,
        max_combo: 0,
        grades: (0..count).map(|index| (index as u32, 1)).collect(),
        timing: TimingRecord::default(),
    }
}
fn comparisons() -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: vec![
            GhostSnapshot {
                kind: OpponentKind::Own,
                label: "own.bkr".into(),
                hits: u64::MAX,
                misses: 0,
                combo: u64::MAX,
                max_combo: u64::MAX,
                recorded_until: Some(Timestamp::from_nanos(i64::MIN)),
            },
            GhostSnapshot {
                kind: OpponentKind::Other,
                label: "other.bkr".into(),
                hits: 0,
                misses: u64::MAX,
                combo: 0,
                max_combo: 0,
                recorded_until: None,
            },
        ],
        network: Some(NetworkSnapshot {
            status: NetworkStatus::Disconnected,
            progress: Some(Progress {
                song_ns: 9_007_199_254_740_993,
                hits: u64::MAX,
                misses: 0,
                combo: 0,
                max_combo: u64::MAX,
            }),
        }),
    }
}
fn label(view: &HistoricalRecordPresentation, text: &str) {
    let mut scene = Scene::new(960, 720);
    view.compose(&mut scene).unwrap();
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
        "missing stored caption {text}"
    );
}
fn packet(view: &HistoricalRecordPresentation) -> Vec<([f32; 4], [f32; 4], [f32; 4])> {
    let mut scene = Scene::new(960, 720);
    view.compose(&mut scene).unwrap();
    scene
        .rectangles()
        .iter()
        .map(|rect| (rect.bounds, rect.uv, rect.color))
        .collect()
}
#[test]
fn unavailable_known_none_empty_and_selected_comparisons_have_distinct_bounded_page_counts() {
    let score = score(5);
    let none = None;
    let empty = Some(CompetitionSnapshot {
        ghosts: vec![],
        network: None,
    });
    let selected = Some(comparisons());
    for (input, pages) in [
        (None, 2),
        (Some(&none), 3),
        (Some(&empty), 3),
        (Some(&selected), 5),
    ] {
        assert_eq!(historical_page_count(Some(&score), input), pages);
        let mut view = HistoricalRecordPresentation::from_record_with_comparisons(
            value(),
            Some(&score),
            input,
        )
        .unwrap();
        assert_eq!(view.grade_page_count(), pages);
        label(
            &view,
            if input.is_none() {
                "STORED COMPARISONS UNAVAILABLE"
            } else {
                "STORED COMPARISON METADATA"
            },
        );
        if pages == 3 {
            view.set_grade_page(2).unwrap();
            label(
                &view,
                if input.unwrap().is_none() {
                    "NO SELECTED COMPARISONS"
                } else {
                    "STORED COMPARISON SNAPSHOT EMPTY"
                },
            );
            label(&view, "STORED COMPARISONS PAGE 1 / 1");
        }
    }
    assert_eq!(
        HistoricalRecordPresentation::from_record(value(), Some(&score))
            .unwrap()
            .grade_page_count(),
        2
    );
}
#[test]
fn ghost_and_peer_pages_preserve_exact_opaque_integers_and_recorded_prefix_provenance() {
    let selected = Some(comparisons());
    let mut view = HistoricalRecordPresentation::from_record_with_comparisons(
        value(),
        Some(&score(5)),
        Some(&selected),
    )
    .unwrap();
    view.set_grade_page(2).unwrap();
    label(&view, "own.bkr");
    label(&view, "18446744073709551615");
    label(&view, "-9223372036854775808");
    label(&view, "SAVED REPLAY OPERATION PREFIX");
    label(&view, "STORED OWNER OWN");
    label(&view, "PREFIX DOES NOT PROVE WHOLE-SONG COMPLETION");
    view.set_grade_page(3).unwrap();
    label(&view, "other.bkr");
    label(&view, "18446744073709551615");
    label(&view, "STORED OWNER OTHER");
    label(&view, "RECORDED UNTIL UNAVAILABLE");
    view.set_grade_page(4).unwrap();
    label(&view, "9007199254740993");
    label(&view, "18446744073709551615");
    label(&view, "PEER-REPORTED NOT FINAL RANKING");
    assert_eq!(view.value().0, PlayerId(u32::MAX));
    assert_eq!(
        view.value().1.outcome,
        PlayResultOutcome::BelowClearThreshold
    );
}
#[test]
fn all_network_states_and_unknown_progress_are_preserved_as_peer_metadata_pages() {
    for status in [
        NetworkStatus::Waiting,
        NetworkStatus::Connected,
        NetworkStatus::Disconnected,
        NetworkStatus::Stopped,
    ] {
        let snapshot = Some(CompetitionSnapshot {
            ghosts: vec![],
            network: Some(NetworkSnapshot {
                status,
                progress: None,
            }),
        });
        let mut view = HistoricalRecordPresentation::from_record_with_comparisons(
            value(),
            None,
            Some(&snapshot),
        )
        .unwrap();
        assert_eq!(view.grade_page_count(), 2);
        view.set_grade_page(1).unwrap();
        label(
            &view,
            match status {
                NetworkStatus::Waiting => "WAITING",
                NetworkStatus::Connected => "CONNECTED",
                NetworkStatus::Disconnected => "DISCONNECTED",
                NetworkStatus::Stopped => "STOPPED",
            },
        );
        label(&view, "PEER PROGRESS UNAVAILABLE");
        label(&view, "PEER-REPORTED NOT FINAL RANKING");
    }
}
#[test]
fn maximum_1033_pages_retains_score_allocation_and_reuses_comparison_packets_atomically() {
    let score = score(4096);
    let mut selected = comparisons();
    selected.ghosts = vec![selected.ghosts[0].clone(); 8];
    let selected = Some(selected);
    let mut view = HistoricalRecordPresentation::from_record_with_comparisons(
        value(),
        Some(&score),
        Some(&selected),
    )
    .unwrap();
    assert_eq!(view.grade_page_count(), 1033);
    let grades = view.score().unwrap().grades.as_ptr();
    view.set_grade_page(1024).unwrap();
    let ghost = packet(&view);
    view.set_grade_page(1032).unwrap();
    let peer = packet(&view);
    assert_ne!(ghost, peer);
    assert_eq!(view.score().unwrap().grades.as_ptr(), grades);
    assert!(!view.set_grade_page(1032).unwrap());
    assert_eq!(packet(&view), peer);
    for page in [1033, usize::MAX] {
        assert!(view.set_grade_page(page).is_err());
        assert_eq!(view.grade_page(), 1032);
        assert_eq!(packet(&view), peer);
    }
    view.set_grade_page(1024).unwrap();
    assert_eq!(packet(&view), ghost);
}
