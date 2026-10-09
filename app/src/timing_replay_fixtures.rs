use super::*;
use crate::{
    play_policy::{ResolvedPlayPolicy, TimingPresetSelection},
    replay_capture::LiveReplayCapture,
};
use beatkernel::{
    audio::command_queue,
    input::*,
    runtime::{Runtime, RuntimeProcessingClock},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint},
    transport::{Rate, Transport},
};
use beatkernel_bms::{BmsGaugeKind, BmsRankPrecedence, BmsTimingPreset};

fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn source() -> BmsChart {
    beatkernel_bms::parse(
        "#BPM 120\n#RANK 3\n#WAV01 note.wav\n#00151:0101\n#00156:0101\n",
        Default::default(),
    )
    .unwrap()
}
fn policy(source: &BmsChart) -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::bms_with_timing(
        source,
        BmsGaugeKind::Groove,
        TimingPresetSelection {
            preset: BmsTimingPreset::BeatorajaSevenKeys8320241dV1,
            precedence: BmsRankPrecedence::RankFirst,
        },
        5_000_000,
    )
    .unwrap()
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
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(17),
        timestamp: Timestamp::from_nanos(ns),
    }
}

fn recorded(
    mode: BmsInputMode,
    start: Timestamp,
    end: Option<Timestamp>,
) -> (ReplayFile, Vec<beatkernel::judge::JudgeEvent>) {
    let source = source();
    let policy = policy(&source);
    let selected = crate::section_start::source_at(&source, start).unwrap();
    let judge = crate::mine_plan::prepare_judge_with_timing(
        &selected,
        selected.compile().unwrap().chart,
        policy.judge().clone(),
        mode,
        1024,
        Some(policy.timing().unwrap().profiles()),
    )
    .unwrap();
    let mut capture = LiveReplayCapture::new_with_policy(
        &judge,
        ClockDomainId(17),
        limits(),
        start,
        0,
        end,
        mode,
        None,
        &policy,
    )
    .unwrap();
    let bindings = BindingMap::from_bindings([0x11u32, 0x16].map(|control| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(control as u16),
        game_control: GameControlId(control),
    }))
    .unwrap();
    let (producer, _consumer) = command_queue(32).unwrap();
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
    let mut events = vec![];
    for (ns, state) in [
        (2_020_000_000, ButtonState::Down),
        (3_120_000_000, ButtonState::Up),
    ] {
        for control in [0x11u32, 0x16] {
            let event = PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(9), point(ns), events.len() as u64),
                control: PhysicalControlId::keyboard(control as u16),
                state,
            });
            let report = runtime.process_input(event, &Identity, point(ns)).unwrap();
            assert_eq!(report.judge_events.len(), 1);
            capture.record_report(&report).unwrap();
            events.extend(report.judge_events);
        }
    }
    (capture.into_file(), events)
}

#[test]
fn staged_live_capture_replay_visual_and_repeated_seek_preserve_actual_results() {
    for mode in [BmsInputMode::ButtonOnly, BmsInputMode::ButtonOrContact] {
        for (start, end) in [
            (Timestamp::ZERO, None),
            (
                Timestamp::from_nanos(1_000_000_000),
                Some(Timestamp::from_nanos(4_000_000_000)),
            ),
        ] {
            let source = source();
            let policy = policy(&source);
            let (file, events) = recorded(mode, start, end);
            let bytes = encode_replay(&file, limits()).unwrap();
            let decoded = decode_replay(&bytes, limits()).unwrap();
            let setup = decode_section_setup(&decoded.header.options).unwrap();
            assert_eq!(setup.timing.as_ref(), policy.timing());
            assert_eq!(setup.start, start);
            assert_eq!(setup.end, end);
            assert_eq!(setup.input_mode, mode);
            let pristine = validate_section_setup(&source, &decoded, limits()).unwrap();
            policy.validate_timing(&pristine, mode).unwrap();
            let mut replay = reconstruct_section(&source, decoded.clone(), limits()).unwrap();
            assert_eq!(replay.results(), events);
            let final_hash = replay.engine().stable_hash().unwrap();
            for _ in 0..3 {
                replay.seek(Timestamp::from_nanos(2_500_000_000)).unwrap();
                assert_eq!(replay.results(), &events[..2]);
                replay.seek(Timestamp::from_nanos(3_120_000_000)).unwrap();
                assert_eq!(replay.results(), events);
                assert_eq!(replay.engine().stable_hash().unwrap(), final_hash);
            }
            let mut visual =
                crate::replay_visual::ReplayVisual::new_section(&source, &decoded, limits())
                    .unwrap();
            assert_eq!(
                visual
                    .advance_to(Timestamp::from_nanos(3_120_000_000))
                    .unwrap(),
                events
            );
            assert!(decode_profile(&decoded.header.options).is_err());
            assert!(decode_setup(&decoded.header.options).is_err());
            assert!(decode_chart_setup(&decoded.header.options).is_err());
            assert!(reconstruct(&source, decoded, limits()).is_err());
        }
    }
}

#[test]
fn staged_recording_rejects_wrong_declaration_rule_schema_and_routing_profile() {
    let source = source();
    let (file, _) = recorded(BmsInputMode::ButtonOnly, Timestamp::ZERO, None);
    let mut changed_source = source.clone();
    changed_source.metadata.insert("RANK".into(), "2".into());
    assert!(matches!(
        validate_section_setup(&changed_source, &file, limits()),
        Err(PlaybackError::IdentityMismatch(
            "declared timing difficulty"
        ))
    ));
    let mut wrong_schema = file.clone();
    wrong_schema.header.rules_identity = b"beatkernel-bms/builtin-judge/v1".to_vec();
    assert!(matches!(
        validate_section_setup(&source, &wrong_schema, limits()),
        Err(PlaybackError::IdentityMismatch("BMS rule schema"))
    ));
    let (inner, timing) = crate::replay_timing_policy::split_options(&file.header.options).unwrap();
    let policy = policy(&source);
    let legacy = crate::mine_plan::prepare_judge(
        &source,
        source.compile().unwrap().chart,
        policy.judge().clone(),
        BmsInputMode::ButtonOnly,
        1024,
    )
    .unwrap();
    assert!(LiveReplayCapture::new_with_policy(
        &legacy,
        ClockDomainId(17),
        limits(),
        Timestamp::ZERO,
        0,
        None,
        BmsInputMode::ButtonOnly,
        None,
        &policy
    )
    .is_err());
    let mut no_timing = file.clone();
    no_timing.header.options = inner.to_vec();
    assert!(validate_section_setup(&source, &no_timing, limits()).is_err());
    assert!(timing.is_some());
}
