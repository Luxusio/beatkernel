//! Display-only record projection from genuine captured Runtime reports.
use super::*;
use crate::{
    gauge::{BmsGauge, GaugeProfile},
    judgment_policy::{BmsJudgmentPolicy, GradeClass},
    play_result::CompletedPlayResult,
    record_catalog::RecordPreview,
    replay_capture::LiveReplayCapture,
    replay_playback::RecordedSetup,
    result_archive::{ArchivedScore, ResultArchive},
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration},
    transport::{Rate, Transport},
};
use std::sync::Arc;

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
pub(crate) fn record() -> RecordPreview {
    record_with_archive().0
}
pub(crate) fn record_with_archive() -> (RecordPreview, ResultArchive) {
    let source = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 note.wav\n#00011:01\n#00111:01\n",
        Default::default(),
    )
    .unwrap();
    let setup = RecordedSetup {
        timing: None,
        judgments: Some(
            BmsJudgmentPolicy::new(&[GradeClass {
                grade: JudgeGrade(1),
                class: beatkernel_bms::BmsJudgment::PGreat,
            }])
            .unwrap(),
        ),
        profile: JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(5),
                late: Duration::from_nanos(5),
            }],
            Duration::ZERO,
        )
        .unwrap(),
        gauge: GaugeProfile::default(),
        start: Timestamp::ZERO,
        chart_seed: u64::MAX,
        end: None,
        input_mode: beatkernel_bms::BmsInputMode::ButtonOnly,
    };
    let judge = JudgeEngine::new(
        source.compile().unwrap().chart,
        source.rules(),
        setup.profile.clone(),
    )
    .unwrap();
    let limits = crate::competition_live::replay_limits().unwrap();
    let mut capture = LiveReplayCapture::new_with_gauge(
        &judge,
        ClockDomainId(17),
        limits,
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
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        vec![],
        0,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    capture
        .record_report(
            &runtime
                .process_input(
                    PhysicalInputEvent::Button(ButtonEvent {
                        meta: EventMeta::new(DeviceId(9), point(2), 0),
                        control: PhysicalControlId::keyboard(7u16),
                        state: ButtonState::Down,
                    }),
                    &Exact,
                    point(2),
                )
                .unwrap(),
        )
        .unwrap();
    capture
        .record_report(&runtime.advance_to(point(3), &Exact, point(3)).unwrap())
        .unwrap();
    let mut file = capture.into_file();
    file.header =
        crate::replay_judgment_policy::wrap_header(file.header, setup.judgments.as_ref(), limits)
            .unwrap();
    let path = std::path::Path::new("records/曲.bkr");
    let prefix =
        RecordPreview::from_file_with_setup(path, &source, &setup, file.clone(), None, None)
            .unwrap();
    let player = PlayerId(u32::MAX);
    let gauge = BmsGauge::new(setup.gauge.clone());
    let result = CompletedPlayResult::from_completed(Timestamp::ZERO, None, &gauge);
    let mut archive = ResultArchive::from_completed_with_scores(
        &[(player, result)],
        &[(player, file.header.clone(), setup.gauge.clone())],
        &[(player, &prefix.score)],
    )
    .unwrap();
    let comparison = crate::competition_presentation::CompetitionSnapshot {
        ghosts: vec![crate::competition_presentation::GhostSnapshot {
            kind: crate::competition::OpponentKind::Own,
            label: "曲.bkr".into(),
            hits: prefix.score.hits,
            misses: prefix.score.misses,
            combo: prefix.score.combo,
            max_combo: prefix.score.max_combo,
            recorded_until: prefix.recorded_until,
        }],
        network: None,
    };
    archive
        .attach_comparisons(&[(player, Some(&comparison))])
        .unwrap();
    let preview = RecordPreview::from_file_with_setup(
        path,
        &source,
        &setup,
        file,
        Some(&archive),
        Some(player),
    )
    .unwrap();
    (preview, archive)
}

#[test]
fn frozen_record_retains_actual_timing_class_prefix_and_original_history_arcs() {
    let record = record();
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    assert_eq!(
        frozen.score,
        ArchivedScore::from_summary(&record.score).unwrap()
    );
    assert_eq!(frozen.score.timing.count, 1);
    assert_eq!(frozen.score.timing.last, Some(2));
    assert_eq!(frozen.recorded_until, Some(Timestamp::from_nanos(3)));
    assert_eq!(frozen.bms_score, record.bms_score);
    assert_eq!(frozen.historical_bms_score, record.historical_bms_score);
    assert!(Arc::ptr_eq(
        frozen.historical_score.as_ref().unwrap(),
        record.historical_score.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        frozen.historical_comparison.as_ref().unwrap(),
        record.historical_comparison.as_ref().unwrap()
    ));
    assert_eq!(frozen.historical, record.historical);
    frozen.validate().unwrap();
}

#[test]
fn source_draft_and_cow_history_mutation_cannot_change_frozen_record() {
    let mut record = record();
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    let original = frozen.score.clone();
    let history = frozen.historical_score.clone().unwrap();
    record.path = "different.bkr".into();
    record.score.hits = 999;
    record.archive_error = Some("new archive failure".into());
    Arc::make_mut(record.historical_score.as_mut().unwrap()).hits = 999;
    Arc::make_mut(record.historical_comparison.as_mut().unwrap())
        .as_mut()
        .unwrap()
        .ghosts[0]
        .label = "changed.bkr".into();
    assert_eq!(frozen.path, PathBuf::from("records/曲.bkr"));
    assert_eq!(frozen.score, original);
    assert_eq!(history.hits, 1);
    assert!(frozen.archive_error.is_none());
    assert_eq!(
        frozen
            .historical_comparison
            .as_ref()
            .unwrap()
            .as_ref()
            .as_ref()
            .unwrap()
            .ghosts[0]
            .label,
        "曲.bkr"
    );
}

#[test]
fn malformed_frozen_timing_class_path_and_history_are_explicit_refusals() {
    let frozen = FrozenRecordPreview::from_record(&record()).unwrap();
    let mut cases = Vec::new();
    let mut bad = frozen.clone();
    bad.score.timing.late = 0;
    cases.push(bad);
    let mut bad = frozen.clone();
    bad.score.combo = 2;
    cases.push(bad);
    let mut bad = frozen.clone();
    bad.path = "bad\npath.bkr".into();
    cases.push(bad);
    let mut bad = frozen.clone();
    bad.bms_score.as_mut().unwrap().ex_score += 1;
    cases.push(bad);
    let mut bad = frozen.clone();
    bad.historical = None;
    cases.push(bad);
    for bad in cases {
        assert!(bad.validate().is_err());
    }
    frozen.validate().unwrap();
}

#[test]
fn valid_prefix_survives_archive_diagnostic_and_empty_timing_remains_read_only() {
    let mut record = record();
    record.archive_error = Some("sidecar association failed".into());
    record.historical = None;
    record.historical_score = None;
    record.historical_comparison = None;
    record.historical_bms_score = None;
    let frozen = FrozenRecordPreview::from_record(&record).unwrap();
    assert_eq!(frozen.score.hits, 1);
    assert_eq!(frozen.score.misses, 0);
    assert_eq!(
        frozen.archive_error.as_deref(),
        Some("sidecar association failed")
    );
    assert_eq!(
        crate::timing_display::record(&frozen.score.timing),
        crate::timing_display::summary(&record.score.timing)
    );
    assert_eq!(
        crate::timing_display::record(&crate::timing::TimingRecord::default()),
        ("BIAS --".into(), "MEAN ABS --".into())
    );
}
