//! Real Runtime capture and portable recorded-setup preview; no browser native host.
use super::*;
use crate::{
    competition::OpponentKind,
    competition_presentation::{CompetitionSnapshot, GhostSnapshot},
    gauge::{BmsGauge, GaugeProfile},
    judgment_policy::{BmsJudgmentPolicy, GradeClass},
    local_players::PlayerId,
    play_result::CompletedPlayResult,
    replay_capture::LiveReplayCapture,
    replay_playback::decode_section_setup,
    result_archive::ResultArchive,
    settings::SettingsHost,
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::JudgeEngine,
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint},
    transport::{Rate, Transport},
};
use beatkernel_bms::BmsJudgment;

fn source() -> beatkernel_bms::BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#WAV01 note.wav\n#00011:01\n#00111:01\n",
        Default::default(),
    )
    .unwrap()
}
fn native(gauge: &str) -> NativeSettings {
    NativeSettings::from_args(
        &[
            "--gauge".into(),
            gauge.into(),
            "--chart-seed".into(),
            u64::MAX.to_string(),
        ],
        SettingsHost::Linux,
    )
    .unwrap()
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Exact;
impl ClockMapper for Exact {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn recording(source: &beatkernel_bms::BmsChart, setup: &RecordedSetup) -> ReplayFile {
    let section = crate::section_start::source_at(source, setup.start).unwrap();
    let judge = JudgeEngine::new(
        section.compile().unwrap().chart,
        section.rules(),
        setup.profile.clone(),
    )
    .unwrap();
    let mut capture = LiveReplayCapture::new_with_gauge(
        &judge,
        ClockDomainId(17),
        replay_limits().unwrap(),
        setup.start,
        setup.chart_seed,
        setup.end,
        setup.input_mode,
        None,
        &setup.gauge,
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(7u16),
        game_control: GameControlId(0x11),
    }])
    .unwrap();
    let (producer, _consumer) = command_queue(8).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(17),
        ClockDomainId(17),
        Transport::new(Timestamp::ZERO, setup.start, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    let report = runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(9), point(0), 0),
                control: PhysicalControlId::keyboard(7u16),
                state: ButtonState::Down,
            }),
            &Exact,
            point(0),
        )
        .unwrap();
    capture.record_report(&report).unwrap();
    capture
        .record_report(&runtime.advance_to(point(1), &Exact, point(1)).unwrap())
        .unwrap();
    let mut file = capture.into_file();
    file.header = crate::replay_judgment_policy::wrap_header(
        file.header,
        setup.judgments.as_ref(),
        replay_limits().unwrap(),
    )
    .unwrap();
    file
}
fn pure(
    source: &beatkernel_bms::BmsChart,
    setup: &RecordedSetup,
    file: ReplayFile,
    archive: Option<&ResultArchive>,
    player: Option<PlayerId>,
) -> Result<RecordPreview, String> {
    RecordPreview::from_file_with_setup(
        Path::new("曲/prefix.bkr"),
        source,
        setup,
        file,
        archive,
        player,
    )
}

#[test]
fn portable_setup_and_native_facade_have_identical_actual_runtime_prefixes() {
    let source = source();
    for gauge in ["beatkernel", "groove", "hard"] {
        let settings = native(gauge);
        let setup = draft_section(&settings, &source).unwrap();
        let file = recording(&source, &setup);
        assert_eq!(decode_section_setup(&file.header.options).unwrap(), setup);
        let portable = pure(&source, &setup, file.clone(), None, None).unwrap();
        let native = RecordPreview::from_file_with_archive(
            Path::new("曲/prefix.bkr"),
            &source,
            &settings,
            file,
            None,
            None,
        )
        .unwrap();
        assert_eq!(portable.score, native.score);
        assert_eq!(portable.records, native.records);
        assert_eq!(portable.recorded_until, native.recorded_until);
        assert_eq!(portable.start, native.start);
        assert_eq!(portable.end, native.end);
        assert_eq!(portable.bms_score, native.bms_score);
        assert_eq!(portable.score.hits, 1);
        assert_eq!(
            portable.score.misses, 0,
            "preview does not manufacture misses after the actual captured prefix"
        );
    }
}

#[test]
fn every_portable_policy_dimension_mismatch_refuses_genuine_prefix() {
    let source = source();
    let setup = draft_section(&native("groove"), &source).unwrap();
    let file = recording(&source, &setup);
    let original = pure(&source, &setup, file.clone(), None, None).unwrap();
    let mut cases = Vec::new();
    let mut changed = setup.clone();
    changed.chart_seed -= 1;
    cases.push(changed);
    let mut changed = setup.clone();
    changed.start = Timestamp::from_nanos(1);
    cases.push(changed);
    let mut changed = setup.clone();
    changed.end = Some(Timestamp::from_nanos(72_000_000_000_000));
    cases.push(changed);
    let mut changed = setup.clone();
    changed.input_mode = beatkernel_bms::BmsInputMode::ButtonOrContact;
    cases.push(changed);
    let mut changed = setup.clone();
    changed.profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(1),
            late: Duration::from_nanos(1),
        }],
        Duration::ZERO,
    )
    .unwrap();
    cases.push(changed);
    let mut changed = setup.clone();
    changed.gauge = GaugeProfile::default();
    cases.push(changed);
    let mut changed = setup.clone();
    changed.judgments = None;
    cases.push(changed);
    let mut changed = setup.clone();
    changed.judgments = Some(
        BmsJudgmentPolicy::new(
            &setup
                .profile
                .windows()
                .iter()
                .map(|window| GradeClass {
                    grade: window.grade,
                    class: BmsJudgment::Great,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    cases.push(changed);
    for changed in cases {
        assert!(pure(&source, &changed, file.clone(), None, None).is_err());
    }
    assert_eq!(
        pure(&source, &setup, file, None, None).unwrap().score,
        original.score
    );
}

#[test]
fn portable_long_finite_sections_preserve_original_song_coordinates_and_prefix_extent() {
    let source = source();
    for (start, end) in [
        (0, 72_000_000_000_000),
        (72_000_000_000_000, 604_800_000_000_001),
    ] {
        let mut setup = draft_section(&native("beatkernel"), &source).unwrap();
        setup.start = Timestamp::from_nanos(start);
        setup.end = Some(Timestamp::from_nanos(end));
        let file = recording(&source, &setup);
        let decoded = decode_section_setup(&file.header.options).unwrap();
        assert_eq!(decoded, setup);
        let preview = pure(&source, &setup, file, None, None).unwrap();
        assert_eq!(preview.start, Timestamp::from_nanos(start));
        assert_eq!(preview.end, Some(Timestamp::from_nanos(end)));
        assert_eq!(
            preview.recorded_until,
            Some(Timestamp::from_nanos(start + 1))
        );
        assert_eq!(preview.score.misses, 0);
    }
}

#[test]
fn archived_score_and_comparison_remain_separate_from_genuine_preview_prefix() {
    let source = source();
    let setup = draft_section(&native("groove"), &source).unwrap();
    let file = recording(&source, &setup);
    let prefix = pure(&source, &setup, file.clone(), None, None).unwrap();
    let player = PlayerId(u32::MAX);
    let gauge = BmsGauge::new(setup.gauge.clone());
    let completed = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let mut archive = ResultArchive::from_completed_with_scores(
        &[(player, completed)],
        &[(player, file.header.clone(), setup.gauge.clone())],
        &[(player, &prefix.score)],
    )
    .unwrap();
    let comparison = CompetitionSnapshot {
        ghosts: vec![GhostSnapshot {
            kind: OpponentKind::Other,
            label: "曲.bkr".into(),
            hits: 9007199254740993,
            misses: 2,
            combo: 3,
            max_combo: 4,
            recorded_until: Some(Timestamp::from_nanos(604_800_000_000_001)),
        }],
        network: None,
    };
    archive
        .attach_comparisons(&[(player, Some(&comparison))])
        .unwrap();
    let inspected = pure(&source, &setup, file.clone(), Some(&archive), Some(player)).unwrap();
    assert_eq!(inspected.score, prefix.score);
    assert_eq!(inspected.historical.unwrap().0, player);
    assert_eq!(
        inspected.historical_score.as_ref().unwrap().hits,
        prefix.score.hits
    );
    assert_eq!(
        inspected.historical_comparison.as_ref().unwrap().as_ref(),
        &Some(comparison)
    );
    assert!(inspected.historical_bms_score.is_some());
    assert!(inspected.archive_error.is_none());
    for requested in [Some(PlayerId(7)), Some(PlayerId(0))] {
        let refused = pure(&source, &setup, file.clone(), Some(&archive), requested).unwrap();
        assert_eq!(refused.score, prefix.score);
        assert_eq!(refused.records, prefix.records);
        assert!(refused.archive_error.is_some());
        assert!(refused.historical.is_none());
    }
    let missing = pure(&source, &setup, file, None, Some(player)).unwrap();
    assert_eq!(missing.score, prefix.score);
    assert!(missing.archive_error.is_some());
}
