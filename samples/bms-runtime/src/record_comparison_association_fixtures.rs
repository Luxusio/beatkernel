//! Deferred actual accepted replay-prefix association and immutable comparison metadata.
use super::*;
use crate::{
    gauge::BmsGauge,
    competition::{ScoreSummary, OpponentKind},
    competition_presentation::{CompetitionSnapshot, GhostSnapshot},
    result_archive::ResultArchive,
    local_players::PlayerId,
    play_result::CompletedPlayResult,
    replay_capture::LiveReplayCapture,
    settings::SettingsHost,
};
use beatkernel::{judge::JudgeEngine, replay::ReplaySession, time::ClockDomainId};
use std::sync::Arc;
fn setup() -> (beatkernel_bms::BmsChart, NativeSettings, ReplayFile) {
    let source = beatkernel_bms::parse("#BPM 60\n", Default::default()).unwrap();
    let settings = NativeSettings::from_args(&[], SettingsHost::Linux).unwrap();
    let setup = draft_section(&settings).unwrap();
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        setup.profile,
    )
    .unwrap();
    let header = LiveReplayCapture::new_section(
        &judge,
        ClockDomainId(17),
        replay_limits().unwrap(),
        setup.start,
        setup.chart_seed,
        setup.end,
    )
    .unwrap()
    .header()
    .clone();
    let mut session = ReplaySession::new(header.clone(), judge).unwrap();
    session
        .advance_to(Timestamp::from_nanos(1_000_000))
        .unwrap();
    (
        source,
        settings,
        ReplayFile::new(header, session.records().to_vec()),
    )
}
fn archive(file: &ReplayFile, ids: &[PlayerId]) -> ResultArchive {
    let gauge = BmsGauge::default();
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let rows: Vec<_> = ids.iter().map(|id| (*id, result)).collect();
    let identities: Vec<_> = ids
        .iter()
        .map(|id| (*id, file.header.clone(), gauge.profile().clone()))
        .collect();
    let score = ScoreSummary {
        hits: 9,
        combo: 9,
        max_combo: 9,
        grades: [(u32::MAX, 9)].into_iter().collect(),
        ..Default::default()
    };
    ResultArchive::from_completed_with_scores(
        &rows,
        &identities,
        &ids.iter().map(|id| (*id, &score)).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn snapshot(label: &str) -> CompetitionSnapshot {
    CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: OpponentKind::Own,
            label: label.into(),
            hits: 4,
            misses: 1,
            combo: 2,
            max_combo: 4,
            recorded_until: Some(Timestamp::from_nanos(i64::MIN)),
        }],
        network: None,
    }
}
#[test]
fn actual_prefix_keeps_legacy_known_none_empty_and_selected_comparison_availability_distinct() {
    let (source, settings, file) = setup();
    for state in 0..4 {
        let mut archive = archive(&file, &[PlayerId(u32::MAX)]);
        let selected = if state == 3 {
            Some(snapshot("saved.bkr"))
        } else if state == 2 {
            Some(CompetitionSnapshot {
                ghosts: vec![],
                network: None,
            })
        } else {
            None
        };
        if state != 0 {
            archive
                .attach_comparisons(&[(PlayerId(u32::MAX), selected.as_ref())])
                .unwrap();
        }
        let preview = RecordPreview::from_file_with_archive(
            Path::new("record.bkr"),
            &source,
            &settings,
            file.clone(),
            Some(&archive),
            Some(PlayerId(u32::MAX)),
        )
        .unwrap();
        assert_eq!(
            (preview.score.hits, preview.score.misses, preview.records),
            (0, 0, 1)
        );
        assert_eq!(preview.historical_score.as_ref().unwrap().hits, 9);
        assert_eq!(preview.historical_comparison.is_some(), state != 0);
        if state != 0 {
            let retained = preview.historical_comparison.as_ref().unwrap();
            assert_eq!(retained.as_ref(), &selected);
            assert!(Arc::ptr_eq(
                retained,
                preview.clone().historical_comparison.as_ref().unwrap()
            ));
        }
    }
}
#[test]
fn identical_replay_headers_select_exact_original_comparison_id_and_ambiguity_preserves_prefix() {
    let (source, settings, file) = setup();
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let mut archive = archive(&file, &ids);
    let low = snapshot("low.bkr");
    let high = snapshot("high.bkr");
    archive
        .attach_comparisons(&[(ids[1], Some(&high)), (ids[0], Some(&low))])
        .unwrap();
    for (id, label) in [(ids[0], "low.bkr"), (ids[1], "high.bkr")] {
        let preview = RecordPreview::from_file_with_archive(
            Path::new("record.bkr"),
            &source,
            &settings,
            file.clone(),
            Some(&archive),
            Some(id),
        )
        .unwrap();
        assert_eq!(preview.historical.unwrap().0, id);
        assert_eq!(
            preview
                .historical_comparison
                .unwrap()
                .as_ref()
                .as_ref()
                .unwrap()
                .ghosts[0]
                .label,
            label
        );
    }
    for id in [None, Some(PlayerId(91))] {
        let preview = RecordPreview::from_file_with_archive(
            Path::new("record.bkr"),
            &source,
            &settings,
            file.clone(),
            Some(&archive),
            id,
        )
        .unwrap();
        assert_eq!(preview.records, 1);
        assert!(preview.archive_error.is_some());
        assert!(preview.historical.is_none());
        assert!(preview.historical_score.is_none());
        assert!(preview.historical_comparison.is_none());
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn incompatible_reassociation_clears_all_historical_attachments_then_valid_retry_restores_them() {
    let (source, settings, file) = setup();
    let mut accepted = archive(&file, &[PlayerId(7)]);
    let snapshot = snapshot("saved.bkr");
    accepted
        .attach_comparisons(&[(PlayerId(7), Some(&snapshot))])
        .unwrap();
    let mut preview = RecordPreview::from_file_with_archive(
        Path::new("record.bkr"),
        &source,
        &settings,
        file.clone(),
        Some(&accepted),
        Some(PlayerId(7)),
    )
    .unwrap();
    let prefix = preview.score.clone();
    let mut wrong_file = file.clone();
    wrong_file.header.seed ^= 1;
    let wrong = archive(&wrong_file, &[PlayerId(7)]);
    preview.attach_archive(&file.header, &wrong, Some(PlayerId(7)));
    assert_eq!(preview.score, prefix);
    assert_eq!(preview.records, 1);
    assert!(preview.archive_error.is_some());
    assert!(preview.historical.is_none());
    assert!(preview.historical_score.is_none());
    assert!(preview.historical_comparison.is_none());
    preview.attach_archive(&file.header, &accepted, Some(PlayerId(7)));
    assert!(preview.archive_error.is_none());
    assert!(preview.historical.is_some());
    assert!(preview.historical_score.is_some());
    assert!(preview.historical_comparison.is_some());
}
