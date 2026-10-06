use super::*;
use crate::{
    gauge::{BmsGauge, GaugeFailure},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::command_queue,
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        PhysicalControlId, PhysicalInputEvent, GameControlId, codec::CodecLimits,
    },
    judge::JudgeOutcome,
    replay::codec::ReplayCodecLimits,
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Timestamp},
    transport::{Transport, Rate},
};
fn windows() -> [ClassifiedWindow; 4] {
    [
        (u32::MAX, BmsJudgment::PGreat),
        (0, BmsJudgment::Great),
        (77, BmsJudgment::Good),
        (7, BmsJudgment::Bad),
    ]
    .map(|(grade, judgment)| ClassifiedWindow {
        judgment,
        window: JudgeWindow {
            grade: JudgeGrade(grade),
            early: Duration::from_nanos(match judgment {
                BmsJudgment::PGreat => 1,
                BmsJudgment::Great => 2,
                BmsJudgment::Good => 3,
                _ => 4,
            }),
            late: Duration::from_nanos(match judgment {
                BmsJudgment::PGreat => 1,
                BmsJudgment::Great => 2,
                BmsJudgment::Good => 3,
                _ => 4,
            }),
        },
    })
}
fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 60\n#WAV01 note.wav\n#TOTAL 320.5\n#00011:01010101",
        Default::default(),
    )
    .unwrap()
}
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, point: ClockPoint, target: ClockDomainId) -> Option<Timestamp> {
        (point.domain == target).then_some(point.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
#[test]
fn explicit_classes_reach_actual_judge_capture_replay_and_all_six_gauges() {
    let source = source();
    let limits =
        ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap();
    for kind in BmsGaugeKind::ALL {
        let resolved = ResolvedPlayPolicy::bms(&source, kind, &windows(), -19).unwrap();
        assert_eq!(resolved.selection(), GaugeSelection::Bms(kind));
        assert_eq!(resolved.total(), Some(source.gauge_total()));
        let engine = crate::mine_plan::prepare_judge(
            &source,
            source.compile().unwrap().chart,
            resolved.judge().clone(),
            beatkernel_bms::BmsInputMode::ButtonOnly,
            1024,
        )
        .unwrap();
        let mut capture = LiveReplayCapture::new_with_gauge(
            &engine,
            ClockDomainId(17),
            limits,
            Timestamp::ZERO,
            0,
            None,
            beatkernel_bms::BmsInputMode::ButtonOnly,
            None,
            resolved.gauge(),
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
            engine,
            producer,
            vec![],
            0,
        )
        .unwrap();
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        let mut gauge = BmsGauge::new(resolved.gauge().try_copy().unwrap());
        let mut events = vec![];
        for (index, entry) in windows().iter().enumerate() {
            let ns = index as i64 * 1_000_000_000 + index as i64 + 1 + 19;
            for (offset, state) in [ButtonState::Down, ButtonState::Up].into_iter().enumerate() {
                let input = PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(9), point(ns), (index * 2 + offset) as u64),
                    control: PhysicalControlId::keyboard(7u16),
                    state,
                });
                let report = runtime.process_input(input, &Identity, point(ns)).unwrap();
                if offset == 0 {
                    assert!(
                        matches!(report.judge_events[0].outcome,JudgeOutcome::Hit {grade,..} if grade==entry.window.grade)
                    );
                }
                gauge
                    .observe(&report.judge_events, &report.hazard_events)
                    .unwrap();
                capture.record_report(&report).unwrap();
                events.extend(report.judge_events);
            }
        }
        let file = capture.into_file();
        let setup = crate::replay_playback::decode_section_setup(&file.header.options).unwrap();
        assert_eq!(&setup.profile, resolved.judge());
        assert_eq!(&setup.gauge, resolved.gauge());
        let mut visual =
            crate::replay_visual::ReplayVisual::new_section(&source, &file, limits).unwrap();
        assert_eq!(
            visual
                .advance_to(Timestamp::from_nanos(3_000_000_023))
                .unwrap(),
            events
        );
        assert_eq!(visual.gauge(), &gauge);
        if kind == BmsGaugeKind::Hazard {
            assert_eq!(gauge.snapshot().failure, Some(GaugeFailure::Depleted));
        }
    }
}
#[test]
fn builtin_and_exact_selection_names_preserve_legacy_policy_and_invalid_aliases_refuse() {
    for (name, selection) in [
        ("beatkernel", GaugeSelection::BeatKernel),
        ("assist-easy", GaugeSelection::Bms(BmsGaugeKind::AssistEasy)),
        ("easy", GaugeSelection::Bms(BmsGaugeKind::Easy)),
        ("groove", GaugeSelection::Bms(BmsGaugeKind::Groove)),
        ("hard", GaugeSelection::Bms(BmsGaugeKind::Hard)),
        ("ex-hard", GaugeSelection::Bms(BmsGaugeKind::ExHard)),
        ("hazard", GaugeSelection::Bms(BmsGaugeKind::Hazard)),
    ] {
        assert_eq!(name.parse::<GaugeSelection>().unwrap(), selection);
    }
    for name in ["", "Hard", "hard ", "asio", "lr2", "exhard"] {
        assert!(name.parse::<GaugeSelection>().is_err());
    }
    let policy = ResolvedPlayPolicy::builtin(0, i64::MAX, i64::MIN).unwrap();
    assert_eq!(
        policy.judge().windows(),
        &[JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::from_nanos(i64::MAX)
        }]
    );
    assert_eq!(
        policy.judge().input_offset(),
        Duration::from_nanos(i64::MIN)
    );
    assert_eq!(policy.gauge(), &GaugeProfile::default());
    assert_eq!(policy.total(), None);
    let (judge, gauge) = policy.into_parts();
    assert_eq!(judge.windows()[0].grade, JudgeGrade(1));
    assert_eq!(gauge, GaugeProfile::default());
    assert!(ResolvedPlayPolicy::builtin(-1, 0, 0).is_err());
}
#[test]
fn unsupported_hit_classes_grade_duplicates_capacity_and_empty_source_refuse() {
    let source = source();
    let mut bad = windows();
    for class in [BmsJudgment::Poor, BmsJudgment::EmptyPoor] {
        bad[0].judgment = class;
        assert!(ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &bad, 0).is_err());
    }
    bad = windows();
    bad[1].window.grade = bad[0].window.grade;
    assert!(ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &bad, 0).is_err());
    bad = windows();
    bad[1].window.late = Duration::ZERO;
    assert!(ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &bad, 0).is_err());
    assert!(ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &[], 0).is_err());
    assert!(
        ResolvedPlayPolicy::bms(&source, BmsGaugeKind::Groove, &vec![windows()[0]; 65], 0).is_err()
    );
    let empty = beatkernel_bms::parse("#BPM 120", Default::default()).unwrap();
    assert!(ResolvedPlayPolicy::bms(&empty, BmsGaugeKind::Hard, &windows(), 0).is_err());
    let invalid = beatkernel_bms::parse(
        "#BPM 60\n#WAV01 note.wav\n#TOTAL bad\n#00011:01",
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        ResolvedPlayPolicy::bms(&invalid, BmsGaugeKind::Groove, &windows(), 0)
            .unwrap()
            .total()
            .unwrap()
            .source,
        beatkernel_bms::TotalSource::Invalid
    );
}

#[test]
fn native_builtin_boundary_retains_original_judge_error_type_and_text() {
    let config = crate::native_judge::NativeJudgeConfig {
        early: -1,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(17),
        end: None,
    };
    let error = config.profile().unwrap_err();
    assert!(matches!(
        error.downcast_ref::<JudgeError>(),
        Some(JudgeError::InvalidProfile)
    ));
    assert_eq!(error.to_string(), JudgeError::InvalidProfile.to_string());
}
