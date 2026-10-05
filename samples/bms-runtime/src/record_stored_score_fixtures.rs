//! Deferred actual replay-prefix inspection with separate stored final details.
use super::*;
use crate::{
    competition::ScoreSummary,
    gauge::BmsGauge,
    local_players::PlayerId,
    play_result::CompletedPlayResult,
    result_archive::{ResultArchive, ArchivedScore},
    replay_capture::LiveReplayCapture,
    settings::SettingsHost,
};
use beatkernel::{
    input::{
        ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
        GameInputEvent,
    },
    judge::JudgeEngine,
    replay::ReplaySession,
    time::{ClockDomainId, ClockPoint},
};
use std::sync::Arc;
fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#WAV01 tap.wav\n#00011:01\n#00112:01\n",
        Default::default(),
    )
    .unwrap()
}
fn draft(start: i64, end: Option<i64>) -> NativeSettings {
    let mut args = vec![
        "--start-ns".into(),
        start.to_string(),
        "--chart-seed".into(),
        u64::MAX.to_string(),
    ];
    if let Some(end) = end {
        args.extend(["--end-ns".into(), end.to_string()]);
    }
    NativeSettings::from_args(&args, SettingsHost::Linux).unwrap()
}
fn recording(settings: &NativeSettings) -> ReplayFile {
    let source = source();
    let setup = draft_section(settings).unwrap();
    let selected = crate::section_start::source_at(&source, setup.start).unwrap();
    let judge = JudgeEngine::new(
        selected.compile().unwrap().chart,
        selected.rules(),
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
    if setup.start == Timestamp::ZERO {
        session
            .push_input(
                GameInputEvent {
                    game_control: source.notes[0].lane.control(),
                    physical: PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(
                            DeviceId(u64::MAX),
                            ClockPoint {
                                domain: ClockDomainId(17),
                                timestamp: Timestamp::ZERO,
                            },
                            u64::MAX,
                        ),
                        control: PhysicalControlId::keyboard(4),
                        state: ButtonState::Down,
                    }),
                },
                Timestamp::ZERO,
            )
            .unwrap();
        assert_eq!(session.results().len(), 1);
    }
    ReplayFile::new(header, session.records().to_vec())
}
fn final_score(hits: u64) -> ScoreSummary {
    // Public historical policy fixture, deliberately distinct from replay prefix.
    ScoreSummary {
        hits,
        combo: hits,
        max_combo: hits,
        grades: [(u32::MAX, hits)].into_iter().collect(),
        ..Default::default()
    }
}
fn archive(
    file: &ReplayFile,
    start: i64,
    end: Option<i64>,
    scored: bool,
    ids: &[PlayerId],
) -> ResultArchive {
    let gauge = BmsGauge::default();
    let result = CompletedPlayResult::from_completed(
        Timestamp::from_nanos(start),
        end.map(Timestamp::from_nanos),
        &gauge,
    );
    let rows: Vec<_> = ids.iter().map(|id| (*id, result)).collect();
    let identities: Vec<_> = ids
        .iter()
        .map(|id| (*id, file.header.clone(), gauge.profile().clone()))
        .collect();
    let summaries: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(index, _)| final_score(index as u64 + 9))
        .collect();
    let scores: Vec<_> = ids
        .iter()
        .zip(&summaries)
        .rev()
        .map(|(id, score)| (*id, score))
        .collect();
    if scored {
        ResultArchive::from_completed_with_scores(&rows, &identities, &scores).unwrap()
    } else {
        ResultArchive::from_completed(&rows, &identities).unwrap()
    }
}
#[test]
fn real_prefix_and_stored_final_details_remain_separate_with_exact_full_and_long_practice_identity()
{
    for (start, end) in [
        (0, None),
        (0, Some(72_000_000_000_000)),
        (72_000_000_000_000, Some(604_800_000_000_000)),
    ] {
        let draft = draft(start, end);
        let file = recording(&draft);
        let archive = archive(&file, start, end, true, &[PlayerId(u32::MAX)]);
        let preview = RecordPreview::from_file_with_archive(
            Path::new("memory.bkr"),
            &source(),
            &draft,
            file.clone(),
            Some(&archive),
            Some(PlayerId(u32::MAX)),
        )
        .unwrap();
        assert_eq!(preview.score.hits, u64::from(start == 0));
        assert_eq!(preview.score.misses, 0);
        assert_eq!(preview.records, file.records.len());
        assert_eq!(preview.start, Timestamp::from_nanos(start));
        assert_eq!(preview.end, end.map(Timestamp::from_nanos));
        assert_eq!(preview.historical.unwrap().0, PlayerId(u32::MAX));
        let stored = preview.historical_score.as_ref().unwrap();
        assert_eq!(
            stored.as_ref(),
            &ArchivedScore::from_summary(&final_score(9)).unwrap()
        );
        assert!(Arc::ptr_eq(
            stored,
            preview.clone().historical_score.as_ref().unwrap()
        ));
        assert!(preview.archive_error.is_none());
    }
}
#[test]
fn version_one_and_absent_archive_never_infer_final_statistics_from_genuine_hit_prefix() {
    let draft = draft(0, None);
    let file = recording(&draft);
    let archive = archive(&file, 0, None, false, &[PlayerId(7)]);
    let legacy = RecordPreview::from_file_with_archive(
        Path::new("memory.bkr"),
        &source(),
        &draft,
        file.clone(),
        Some(&archive),
        Some(PlayerId(7)),
    )
    .unwrap();
    assert_eq!(legacy.score.hits, 1);
    assert!(legacy.historical.is_some());
    assert!(legacy.historical_score.is_none());
    let absent = RecordPreview::from_file_with_archive(
        Path::new("memory.bkr"),
        &source(),
        &draft,
        file,
        None,
        None,
    )
    .unwrap();
    assert_eq!(absent.score.hits, 1);
    assert!(absent.historical.is_none());
    assert!(absent.historical_score.is_none());
}
#[test]
fn equal_genuine_headers_require_original_id_and_select_exact_nonconsecutive_final_score() {
    let draft = draft(0, None);
    let file = recording(&draft);
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let archive = archive(&file, 0, None, true, &ids);
    for (index, id) in ids.into_iter().enumerate() {
        let preview = RecordPreview::from_file_with_archive(
            Path::new("memory.bkr"),
            &source(),
            &draft,
            file.clone(),
            Some(&archive),
            Some(id),
        )
        .unwrap();
        assert_eq!(preview.historical.unwrap().0, id);
        assert_eq!(preview.historical_score.unwrap().hits, index as u64 + 9);
        assert_eq!(preview.score.hits, 1);
    }
    for player in [None, Some(PlayerId(91)), Some(PlayerId(0))] {
        let preview = RecordPreview::from_file_with_archive(
            Path::new("memory.bkr"),
            &source(),
            &draft,
            file.clone(),
            Some(&archive),
            player,
        )
        .unwrap();
        assert_eq!(preview.score.hits, 1);
        assert!(preview.archive_error.is_some());
        assert!(preview.historical.is_none());
        assert!(preview.historical_score.is_none());
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_reassociation_clears_both_attachments_but_preserves_usable_prefix_and_header_mismatch_diagnostic()
 {
    let draft = draft(0, None);
    let file = recording(&draft);
    let accepted = archive(&file, 0, None, true, &[PlayerId(7)]);
    let mut preview = RecordPreview::from_file_with_archive(
        Path::new("memory.bkr"),
        &source(),
        &draft,
        file.clone(),
        Some(&accepted),
        Some(PlayerId(7)),
    )
    .unwrap();
    let prefix = preview.score.clone();
    let mut mismatched = file.clone();
    mismatched.header.seed ^= 1;
    let wrong = archive(&mismatched, 0, None, true, &[PlayerId(7)]);
    preview.attach_archive(&file.header, &wrong, Some(PlayerId(7)));
    assert_eq!(preview.score, prefix);
    assert_eq!(preview.records, file.records.len());
    assert!(preview.historical.is_none());
    assert!(preview.historical_score.is_none());
    assert!(preview.archive_error.is_some());
    preview.attach_archive(&file.header, &accepted, Some(PlayerId(7)));
    assert_eq!(preview.score, prefix);
    assert!(preview.historical_score.is_some());
    assert!(preview.archive_error.is_none());
}
