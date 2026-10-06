//! Deferred real pristine capture and accepted-operation prefix preview.
use super::*;
use crate::{
    replay_capture::LiveReplayCapture, replay_playback::decode_section_setup,
    settings::SettingsHost, local_players::PlayerId,
};
use beatkernel::{
    judge::JudgeEngine,
    replay::{ReplayOperation, ReplayRecord},
    time::ClockDomainId,
};
fn settings(host: SettingsHost, start: i64, end: Option<i64>, seed: u64) -> NativeSettings {
    let mut args = vec![
        "--start-ns".into(),
        start.to_string(),
        "--chart-seed".into(),
        seed.to_string(),
    ];
    if let Some(end) = end {
        args.extend(["--end-ns".into(), end.to_string()]);
    }
    NativeSettings::from_args(&args, host).unwrap()
}
fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 120\n#WAV01 head.wav\n#00011:01\n#00112:01\n",
        Default::default(),
    )
    .unwrap()
}
fn recording(settings: &NativeSettings, times: &[i64]) -> ReplayFile {
    let source = source();
    let setup = draft_section(settings, &source).unwrap();
    let selected = crate::section_start::source_at(&source, setup.start).unwrap();
    let judge = JudgeEngine::new(
        selected.compile().unwrap().chart,
        selected.rules(),
        setup.profile,
    )
    .unwrap();
    let mut file = LiveReplayCapture::new_with_gauge(
        &judge,
        ClockDomainId(17),
        replay_limits().unwrap(),
        setup.start,
        setup.chart_seed,
        setup.end,
        beatkernel_bms::BmsInputMode::ButtonOnly,
        None,
        &setup.gauge,
    )
    .unwrap()
    .into_file();
    file.records = times
        .iter()
        .enumerate()
        .map(|(index, time)| ReplayRecord {
            ordinal: index as u64,
            song_time: Timestamp::from_nanos(*time),
            operation: ReplayOperation::Advance,
        })
        .collect();
    file
}
#[test]
fn every_host_finite_and_unlimited_preview_preserves_long_extent_and_empty_prefix() {
    for host in [
        SettingsHost::Linux,
        SettingsHost::Windows,
        SettingsHost::Macos,
    ] {
        for (start, end) in [
            (0, None),
            (0, Some(72_000_000_000_000)),
            (72_000_000_000_000, Some(604_800_000_000_000)),
        ] {
            let draft = settings(host, start, end, u64::MAX);
            let file = recording(&draft, &[]);
            assert_eq!(
                decode_section_setup(&file.header.options).unwrap().end,
                end.map(Timestamp::from_nanos)
            );
            let preview =
                RecordPreview::from_file(Path::new("prefix.bkr"), &source(), &draft, file).unwrap();
            assert_eq!(preview.start, Timestamp::from_nanos(start));
            assert_eq!(preview.end, end.map(Timestamp::from_nanos));
            assert_eq!(preview.records, 0);
            assert_eq!(preview.recorded_until, None);
            assert_eq!(preview.score.hits, 0);
            assert_eq!(preview.score.misses, 0);
        }
    }
}
#[test]
fn current_draft_extent_seed_profile_and_input_mode_mismatches_refuse_prefix() {
    let draft = settings(SettingsHost::Linux, 0, Some(72_000_000_000_000), 3);
    let file = recording(&draft, &[0]);
    for wrong in [
        settings(SettingsHost::Linux, 0, None, 3),
        settings(SettingsHost::Linux, 1, Some(72_000_000_000_000), 3),
        settings(SettingsHost::Linux, 0, Some(604_800_000_000_000), 3),
        settings(SettingsHost::Linux, 0, Some(72_000_000_000_000), 4),
    ] {
        assert!(
            RecordPreview::from_file(Path::new("prefix.bkr"), &source(), &wrong, file.clone())
                .is_err()
        );
    }
    let mut changed = file.clone();
    let mut setup = decode_section_setup(&changed.header.options).unwrap();
    setup.profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(1),
            late: Duration::from_nanos(1),
        }],
        Duration::ZERO,
    )
    .unwrap();
    let selected = crate::section_start::source_at(&source(), setup.start).unwrap();
    let judge = JudgeEngine::new(
        selected.compile().unwrap().chart,
        selected.rules(),
        setup.profile,
    )
    .unwrap();
    changed.header = crate::replay_capture::setup_section_header(
        &judge,
        ClockDomainId(17),
        replay_limits().unwrap(),
        setup.start,
        setup.chart_seed,
        setup.end,
    )
    .unwrap();
    assert!(RecordPreview::from_file(Path::new("prefix.bkr"), &source(), &draft, changed).is_err());
    let setup = draft_section(&draft, &source()).unwrap();
    let selected = crate::section_start::source_at(&source(), setup.start).unwrap();
    let judge = JudgeEngine::new(
        selected.compile().unwrap().chart,
        selected.rules(),
        setup.profile,
    )
    .unwrap();
    let contact = LiveReplayCapture::new_with_input_mode(
        &judge,
        ClockDomainId(17),
        replay_limits().unwrap(),
        setup.start,
        setup.chart_seed,
        setup.end,
        beatkernel_bms::BmsInputMode::ButtonOrContact,
    )
    .unwrap()
    .into_file();
    assert!(
        RecordPreview::from_file(Path::new("contact-prefix.bkr"), &source(), &draft, contact)
            .is_err()
    );
}
#[test]
fn optional_wrong_or_ambiguous_history_preserves_prefix_scores_and_no_tail_miss() {
    let draft = settings(SettingsHost::Linux, 0, Some(72_000_000_000_000), 0);
    let file = recording(&draft, &[]);
    let history = crate::record_association_fixtures::archive(
        &[(PlayerId(u32::MAX), file.header.clone())],
        0,
        Some(72_000_000_000_000),
    );
    let preview = RecordPreview::from_file_with_archive(
        Path::new("prefix.bkr"),
        &source(),
        &draft,
        file.clone(),
        Some(&history),
        Some(PlayerId(u32::MAX)),
    )
    .unwrap();
    assert_eq!(preview.historical.unwrap().0, PlayerId(u32::MAX));
    assert_eq!(preview.score.misses, 0);
    assert!(preview.archive_error.is_none());
    let refused = RecordPreview::from_file_with_archive(
        Path::new("prefix.bkr"),
        &source(),
        &draft,
        file.clone(),
        Some(&history),
        Some(PlayerId(7)),
    )
    .unwrap();
    assert!(refused.historical.is_none());
    assert!(refused.archive_error.is_some());
    assert_eq!(refused.score, preview.score);
    let ambiguous = crate::record_association_fixtures::archive(
        &[
            (PlayerId(7), file.header.clone()),
            (PlayerId(9), file.header.clone()),
        ],
        0,
        Some(72_000_000_000_000),
    );
    let refused = RecordPreview::from_file_with_archive(
        Path::new("prefix.bkr"),
        &source(),
        &draft,
        file,
        Some(&ambiguous),
        None,
    )
    .unwrap();
    assert!(refused.historical.is_none());
    assert!(refused.archive_error.is_some());
    assert_eq!(refused.score.misses, 0);
    let prefix = RecordPreview::from_file(
        Path::new("truncated-prefix.bkr"),
        &source(),
        &draft,
        recording(&draft, &[160_000_000]),
    )
    .unwrap();
    assert_eq!(prefix.records, 1);
    assert_eq!(
        prefix.recorded_until,
        Some(Timestamp::from_nanos(160_000_000))
    );
    assert_eq!(
        prefix.score.misses, 1,
        "the later 2-second note must not acquire a synthetic tail miss"
    );
}

#[test]
fn selected_gauge_record_comparison_matches_original_source_policy_and_refuses_other_draft() {
    let source = source();
    let mut settings = NativeSettings::from_args(
        &[
            "--gauge".into(),
            "groove".into(),
            "--start-ns".into(),
            "1000000000".into(),
        ],
        SettingsHost::Linux,
    )
    .unwrap();
    let file = recording(&settings, &[]);
    RecordPreview::from_file(Path::new("practice.bkr"), &source, &settings, file.clone()).unwrap();
    let index = settings
        .fields()
        .iter()
        .position(|row| row.flag == "--gauge")
        .unwrap();
    settings.set_value(index, "hard").unwrap();
    assert!(
        RecordPreview::from_file(Path::new("practice.bkr"), &source, &settings, file.clone())
            .is_err()
    );
    settings.set_value(index, "invalid").unwrap();
    assert!(RecordPreview::from_file(Path::new("practice.bkr"), &source, &settings, file).is_err());
}
