//! Genuine Step/Runtime/queue integration; these tests never supply HOST song progress.
use super::*;
use crate::{
    audio_authority::{AudioAuthority, AudioAuthorityConfig, AudioAuthorityEpoch},
    local_input::InputMerger,
    local_players::{PlayerId, ResolvedInputPlan},
    local_runtime::InputResult,
};
use beatkernel::{
    audio::{AudioFormat, PcmLimits, PcmSample, SampleId, VoiceId},
    input::{
        BackendId, Binding, ButtonEvent, ButtonState, CodecLimits, DeviceId, DeviceSelector,
        EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
    },
    judge::{JudgeOutcome, JudgeStage},
    runtime::SoundBinding,
    time::{AffineClockMapper, ClockInterval, ClockMappingQuality, ExtrapolationPolicy},
};
const HOST_ORIGIN: i64 = 10_000_000_000;
const RAW_ORIGIN: i64 = 1_000_000_000;
const LOGICAL_ORIGIN: i64 = 5_000_000_000;
const CHART: &str = "#BPM 3000\n#WAV01 key.wav\n#00011:00010000\n#00012:00000001";
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(delta: i64) -> ClockPoint {
    point(11, HOST_ORIGIN + delta)
}
fn raw(delta: i64) -> ClockPoint {
    point(22, RAW_ORIGIN + delta)
}
fn logical(delta: i64) -> ClockPoint {
    point(33, LOGICAL_ORIGIN + delta)
}
fn config() -> StepGameplayConfig {
    StepGameplayConfig {
        host_origin: host(0),
        output_origin: raw(0),
        preroll: Duration::ZERO,
        early_ns: 0,
        late_ns: 0,
        offset_ns: 0,
        command_capacity: 16,
        bgm_pending: 2,
        bgm_lookahead: Duration::from_nanos(100_000_000),
        telemetry_capacity: 8,
    }
}
fn authority(prediction: bool) -> AudioAuthority {
    AudioAuthority::new(
        AudioAuthorityConfig {
            history_capacity: 8,
            max_observation_age: Duration::from_nanos(1_000_000_000),
            input_extrapolation: if prediction {
                ExtrapolationPolicy::Bounded {
                    before: Duration::ZERO,
                    after: Duration::from_nanos(20_000_000),
                }
            } else {
                ExtrapolationPolicy::Forbid
            },
            max_input_ahead: Duration::from_nanos(if prediction { 20_000_000 } else { 0 }),
        },
        AudioAuthorityEpoch {
            id: 1,
            stream_origin: raw(0),
            logical_origin: logical(0),
            host_domain: ClockDomainId(11),
        },
    )
    .unwrap()
}
fn prepared() -> PreparedBms {
    let source = beatkernel_bms::parse(CHART, Default::default()).unwrap();
    let compiled = source.compile().unwrap();
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, -0.25], limits).unwrap(),
    )
    .unwrap();
    let sounds = compiled
        .chart
        .objects()
        .iter()
        .map(|object| SoundBinding {
            object: object.id,
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(object.id.0),
            gain: 1.0,
        })
        .collect();
    PreparedBms {
        source,
        compiled,
        bank,
        sounds,
        bgm_commands: vec![],
    }
}
fn bindings(device: Option<DeviceId>) -> BindingMap {
    BindingMap::from_bindings(
        [0x11u32, 0x12]
            .into_iter()
            .enumerate()
            .map(|(i, lane)| Binding {
                device: device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
                physical: PhysicalControlId::keyboard(7 + i as u16),
                game_control: GameControlId(lane),
            }),
    )
    .unwrap()
}
fn input(delta: i64, device: u64, key: u16, seq: u64, state: ButtonState) -> PhysicalInputEvent {
    let at = host(delta);
    let mut meta = EventMeta::new(DeviceId(device), at, seq);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(7),
        code: Some(42),
        timestamp: Some(at),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: PhysicalControlId::keyboard(key),
        state,
    })
}
fn merger(devices: Vec<DeviceId>) -> InputMerger {
    InputMerger::new(ClockDomainId(11), host(0), devices, 16).unwrap()
}
fn solo(prediction: bool) -> StepGameplay {
    let (mut step, bank) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        bindings(None),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(prediction),
    )
    .unwrap();
    assert!(bank.get(SampleId(1)).is_some());
    step.activate_audio().unwrap();
    step
}
fn pair(step: &mut StepGameplay) {
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(0),
            target: host(0),
        },
    )
    .unwrap();
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
}
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(65536, 128, 4096, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn state(step: &StepGameplay) -> String {
    format!(
        "{:?}",
        (
            step.song_time(),
            step.score(),
            step.gauge(),
            step.mine_damage(),
            step.failed(),
            step.started,
            step.activated,
            step.audio_authority(),
            step.judge().effective_song_time()
        )
    )
}

#[test]
fn actual_solo_unequal_rate_preserves_input_provenance_raw_schedule_and_logical_capture() {
    let (mut step, _) = StepGameplay::new_audio_section(
        prepared(),
        config(),
        bindings(None),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(false),
    )
    .unwrap();
    assert_eq!(
        step.competition_header(limits(), 0)
            .unwrap()
            .normalized_clock,
        ClockDomainId(33)
    );
    step.configure_capture(limits(), 0).unwrap();
    step.activate_audio().unwrap();
    let anchor = step.runtime.transport_mut().anchor();
    assert_eq!(anchor.host_time, logical(0).timestamp);
    assert_eq!(anchor.rate, Rate::NORMAL);
    pair(&mut step);
    let mut merger = merger(vec![DeviceId(9)]);
    let original = input(10_000_000, 9, 7, 0, ButtonState::Down);
    merger.admit(original.clone(), host(20_000_000)).unwrap();
    step.record_audio_prefix(host(20_000_000)).unwrap();
    let report = step
        .process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
    assert_eq!(report.audio_at, raw(50_000_000));
    assert_eq!(report.input_mapping_quality, ClockMappingQuality::Unknown);
    let normalized = report.input.as_ref().unwrap().meta();
    assert_eq!(normalized.clock_domain, ClockDomainId(33));
    assert_eq!(normalized.timestamp, logical(20_000_000).timestamp);
    assert_eq!(normalized.original_clock_point, Some(host(10_000_000)));
    assert_eq!(normalized.native, original.meta().native);
    assert_eq!(normalized.source, DeviceId(9));
    assert_eq!(normalized.sequence, 0);
    assert_eq!(report.judge_events.len(), 1);
    assert!(matches!(
        report.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
    assert_eq!(step.score().hits, 1);
    assert_eq!(merger.pending(), 0);
    assert!(
        report
            .audio_commands
            .iter()
            .any(|c| matches!(c,AudioCommand::Play {at,..} if *at==raw(50_000_000).timestamp))
    );
    let deadline = step
        .advance_audio_frontier(&mut merger, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(deadline.song_time, Timestamp::from_nanos(40_000_000));
    assert!(deadline.judge_events.is_empty());
    assert_eq!(step.score().misses, 0);
    assert_eq!(step.runtime.transport_mut().anchor(), anchor);
    let batch = step.take_commands(16).unwrap().unwrap();
    assert_eq!(batch.commands, report.audio_commands);
    step.fail();
    let bytes = step.take_replay().unwrap().unwrap();
    let file = beatkernel::replay::codec::decode_replay(&bytes, limits()).unwrap();
    assert_eq!(file.header.normalized_clock, ClockDomainId(33));
    assert_eq!(file.records.len(), 2);
    assert_eq!(file.records[0].song_time, Timestamp::from_nanos(20_000_000));
    let beatkernel::replay::ReplayOperation::Input(saved) = &file.records[0].operation else {
        panic!("expected recorded input")
    };
    assert_eq!(
        saved.physical.meta().original_clock_point,
        Some(host(10_000_000))
    );
}

#[test]
fn warmup_stale_and_uncovered_evidence_retains_original_events_without_runtime_dispatch() {
    let mut step = solo(false);
    let mut merger = merger(vec![DeviceId(9)]);
    let original = input(10_000_000, 9, 7, 0, ButtonState::Down);
    merger.admit(original.clone(), host(20_000_000)).unwrap();
    step.record_audio_prefix(host(20_000_000)).unwrap();
    for observed in 0..2 {
        let before = state(&step);
        assert!(
            step.process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
                .unwrap()
                .is_none()
        );
        assert_eq!(state(&step), before);
        assert_eq!(
            merger.peek_ready(host(20_000_000)).unwrap(),
            Some(&original)
        );
        if observed == 0 {
            step.observe_audio_output(
                1,
                ClockPair {
                    source: raw(0),
                    target: host(0),
                },
            )
            .unwrap();
        }
    }
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
    let before = state(&step);
    assert!(
        step.process_next_audio_input(&mut merger, host(1_020_000_001), raw(50_000_000), None)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&step), before);
    assert_eq!(merger.pending(), 1);
    let report = step
        .process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
    assert_eq!(step.score().hits, 1);
    assert!(
        step.process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
            .unwrap()
            .is_none()
    );
    let later = input(30_000_000, 9, 8, 1, ButtonState::Down);
    merger.admit(later.clone(), host(30_000_000)).unwrap();
    step.record_audio_prefix(host(30_000_000)).unwrap();
    let before = state(&step);
    assert!(
        step.process_next_audio_input(&mut merger, host(30_000_000), raw(70_000_000), None)
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&step), before);
    assert_eq!(merger.peek_ready(host(30_000_000)).unwrap(), Some(&later));
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(60_000_000),
            target: host(30_000_000),
        },
    )
    .unwrap();
    let report = step
        .process_next_audio_input(&mut merger, host(30_000_000), raw(70_000_000), None)
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(60_000_000));
    assert_eq!(step.score().hits, 2);
}

#[test]
fn older_covered_presentation_and_predicted_input_catchup_never_call_runtime_backward() {
    let mut step = solo(true);
    pair(&mut step);
    let mut merger = merger(vec![DeviceId(9)]);
    step.record_audio_prefix(host(19_000_000)).unwrap();
    let report = step
        .advance_audio_frontier(&mut merger, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::ZERO);
    assert_eq!(
        step.audio_authority().unwrap().committed_presentation(),
        Some(logical(0))
    );
    let original = input(30_000_000, 9, 8, 0, ButtonState::Down);
    merger.admit(original, host(30_000_000)).unwrap();
    step.record_audio_prefix(host(30_000_000)).unwrap();
    let report = step
        .process_next_audio_input(&mut merger, host(30_000_000), raw(70_000_000), None)
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(60_000_000));
    assert_eq!(step.score().hits, 1);
    let before = step.score().clone();
    assert!(
        step.advance_audio_frontier(&mut merger, host(30_000_000), raw(70_000_000))
            .unwrap()
            .is_none()
    );
    assert_eq!(step.song_time(), Timestamp::from_nanos(60_000_000));
    assert_eq!(step.score(), &before);
    assert_eq!(
        step.audio_authority().unwrap().committed_presentation(),
        Some(logical(40_000_000))
    );
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(60_000_000),
            target: host(30_000_000),
        },
    )
    .unwrap();
    assert!(
        step.advance_audio_frontier(&mut merger, host(30_000_000), raw(70_000_000))
            .unwrap()
            .is_none()
    );
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(80_000_000),
            target: host(40_000_000),
        },
    )
    .unwrap();
    step.record_audio_prefix(host(40_000_000)).unwrap();
    let report = step
        .advance_audio_frontier(&mut merger, host(40_000_000), raw(90_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(80_000_000));
    assert_eq!(step.score(), &before);
    assert!(!step.failed());
}

#[test]
fn post_report_score_failure_commits_real_operation_once_before_observer_error() {
    let mut step = solo(false);
    pair(&mut step);
    step.score.hits = u64::MAX;
    let mut merger = merger(vec![DeviceId(9)]);
    merger
        .admit(
            input(10_000_000, 9, 7, 0, ButtonState::Down),
            host(20_000_000),
        )
        .unwrap();
    step.record_audio_prefix(host(20_000_000)).unwrap();
    let error = step
        .process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
        .unwrap_err();
    let StepGameplayError::Report {
        report,
        score_error,
        ..
    } = error
    else {
        panic!("expected genuine report failure")
    };
    assert!(score_error.is_some());
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
    assert_eq!(
        step.audio_authority().unwrap().committed_operation(),
        Some(logical(20_000_000))
    );
    assert_eq!(
        step.audio_authority().unwrap().committed_input_host(),
        Some(host(10_000_000))
    );
    assert_eq!(merger.pending(), 0);
    assert!(step.failed());
    let before = state(&step);
    assert!(
        step.process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
            .is_err()
    );
    assert_eq!(state(&step), before);
}

#[test]
fn genuine_pre_report_core_chronology_failure_fences_without_fictional_authority_commit() {
    let mut step = solo(false);
    pair(&mut step);
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    // Seed a real committed core operation ahead of authority to inject a
    // chronology fault at the actual Runtime boundary, rather than invent a report.
    step.runtime
        .solo_mut()
        .unwrap()
        .advance_to(logical(60_000_000), &Identity, raw(70_000_000))
        .unwrap();
    let mut merger = merger(vec![DeviceId(9)]);
    merger
        .admit(
            input(10_000_000, 9, 7, 0, ButtonState::Down),
            host(20_000_000),
        )
        .unwrap();
    step.record_audio_prefix(host(20_000_000)).unwrap();
    assert!(matches!(
        step.process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None),
        Err(StepGameplayError::Runtime(_))
    ));
    assert_eq!(step.audio_authority().unwrap().committed_operation(), None);
    assert_eq!(step.audio_authority().unwrap().committed_input_host(), None);
    assert_eq!(step.score().hits, 0);
    assert!(step.failed());
    let before = state(&step);
    assert!(
        step.process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
            .is_err()
    );
    assert_eq!(state(&step), before);
}

fn local() -> StepLocalGameplay {
    let ids = [PlayerId(7), PlayerId(u32::MAX)];
    let devices = [DeviceId(9), DeviceId(u64::MAX)];
    let (mut step, _) = StepLocalGameplay::new_audio_section(
        prepared(),
        config(),
        ResolvedInputPlan::new(ids.into_iter().zip(devices.map(Some)).collect()).unwrap(),
        devices.into_iter().map(|d| bindings(Some(d))).collect(),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        authority(false),
    )
    .unwrap();
    step.activate_audio().unwrap();
    step
}
fn local_pair(step: &mut StepLocalGameplay) {
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(0),
            target: host(0),
        },
    )
    .unwrap();
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
}
#[test]
fn actual_sparse_local_members_share_output_authority_without_merging_scores_or_capture_domains() {
    let mut step = local();
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        assert_eq!(
            step.competition_header(id, limits(), 0)
                .unwrap()
                .normalized_clock,
            ClockDomainId(33)
        );
        step.configure_capture(id, limits(), 0).unwrap();
    }
    local_pair(&mut step);
    let mut merger = merger(vec![DeviceId(9), DeviceId(u64::MAX), DeviceId(42)]);
    merger
        .admit(
            input(10_000_000, 9, 7, 0, ButtonState::Down),
            host(20_000_000),
        )
        .unwrap();
    merger
        .admit(
            input(10_000_000, u64::MAX, 7, 0, ButtonState::Down),
            host(20_000_000),
        )
        .unwrap();
    step.record_audio_prefix(host(20_000_000)).unwrap();
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        let Some(InputResult::Processed(reports)) = step
            .process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
            .unwrap()
        else {
            panic!("expected routed report")
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, id);
        assert_eq!(
            reports[0].report.song_time,
            Timestamp::from_nanos(20_000_000)
        );
        assert_eq!(
            reports[0]
                .report
                .input
                .as_ref()
                .unwrap()
                .meta()
                .original_clock_point,
            Some(host(10_000_000))
        );
    }
    let reports = step
        .advance_audio_frontier(&mut merger, host(20_000_000), raw(50_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(reports.len(), 2);
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        assert_eq!(step.score(id).unwrap().hits, 1);
        assert_eq!(step.score(id).unwrap().misses, 0);
    }
    let operation = step.audio_authority().unwrap().committed_operation();
    merger
        .admit(
            input(25_000_000, 42, 7, 0, ButtonState::Down),
            host(30_000_000),
        )
        .unwrap();
    step.record_audio_prefix(host(30_000_000)).unwrap();
    step.observe_audio_output(
        1,
        ClockPair {
            source: raw(60_000_000),
            target: host(30_000_000),
        },
    )
    .unwrap();
    assert!(matches!(
        step.process_next_audio_input(&mut merger, host(30_000_000), raw(70_000_000), None)
            .unwrap(),
        Some(InputResult::Ignored {
            device: DeviceId(42)
        })
    ));
    assert_eq!(
        step.audio_authority().unwrap().committed_operation(),
        operation
    );
    step.fail();
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        let bytes = step.take_replay(id).unwrap().unwrap();
        let file = beatkernel::replay::codec::decode_replay(&bytes, limits()).unwrap();
        assert_eq!(file.header.normalized_clock, ClockDomainId(33));
    }
}

#[test]
fn local_partial_deadline_failure_preserves_actual_first_member_prefix_and_commits_once() {
    let mut step = local();
    local_pair(&mut step);
    struct Identity;
    impl ClockMapper for Identity {
        fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
            None
        }
        fn quality(&self) -> ClockMappingQuality {
            ClockMappingQuality::Unknown
        }
    }
    let mut future = input(30_000_000, u64::MAX, 7, 0, ButtonState::Up);
    future.meta_mut().clock_domain = ClockDomainId(33);
    future.meta_mut().timestamp = logical(60_000_000).timestamp;
    step.group_mut()
        .process_input(future, &Identity, raw(70_000_000))
        .unwrap();
    let mut merger = merger(vec![DeviceId(9), DeviceId(u64::MAX)]);
    step.record_audio_prefix(host(20_000_000)).unwrap();
    let error = step
        .advance_audio_frontier(&mut merger, host(20_000_000), raw(50_000_000))
        .unwrap_err();
    let StepLocalGameplayError::Operation {
        group_error,
        reports,
        ..
    } = error
    else {
        panic!("expected partial group error")
    };
    assert!(group_error.is_some());
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PlayerId(7));
    assert_eq!(
        reports[0].report.song_time,
        Timestamp::from_nanos(40_000_000)
    );
    assert_eq!(reports[0].report.judge_events.len(), 1);
    assert_eq!(step.score(PlayerId(7)).unwrap().misses, 1);
    assert_eq!(step.score(PlayerId(u32::MAX)).unwrap().misses, 0);
    assert_eq!(
        step.audio_authority().unwrap().committed_operation(),
        Some(logical(40_000_000))
    );
    assert!(step.failed());
    assert!(
        step.advance_audio_frontier(&mut merger, host(20_000_000), raw(50_000_000))
            .is_err()
    );
    assert_eq!(step.score(PlayerId(7)).unwrap().misses, 1);
}

#[test]
fn audio_mode_forbids_generic_mutators_before_effects_and_legacy_remains_explicit() {
    let mut step = solo(false);
    let before = state(&step);
    let mapper = AffineClockMapper::exact_offset(
        ClockPair {
            source: host(0),
            target: raw(0),
        },
        ClockInterval {
            start: host(0).timestamp,
            end: host(1_000_000_000).timestamp,
        },
    )
    .unwrap();
    assert!(step.activate(host(0)).is_err());
    assert_eq!(state(&step), before);
    assert!(
        step.configure_output_clock(DisciplineConfig::default())
            .is_err()
    );
    assert_eq!(state(&step), before);
    assert!(
        step.observe_output_clock(ClockPair {
            source: raw(0),
            target: host(0)
        })
        .is_err()
    );
    assert_eq!(state(&step), before);
    assert!(step.update_output_clock(host(0)).is_err());
    assert_eq!(state(&step), before);
    assert!(
        step.process_input(
            input(20_000_000, 9, 7, 0, ButtonState::Down),
            &mapper,
            raw(50_000_000)
        )
        .is_err()
    );
    assert_eq!(state(&step), before);
    assert!(
        step.process_input_at(
            input(20_000_000, 9, 7, 0, ButtonState::Down),
            Position2 { x: 1.0, y: 1.0 },
            &mapper,
            raw(50_000_000)
        )
        .is_err()
    );
    assert_eq!(state(&step), before);
    assert!(
        step.advance_to(host(40_000_000), &mapper, raw(50_000_000))
            .is_err()
    );
    assert_eq!(state(&step), before);
    let (mut legacy, _) =
        StepGameplay::new_section(prepared(), config(), bindings(None), Timestamp::ZERO, None)
            .unwrap();
    assert!(legacy.audio_authority().is_none());
    legacy.activate(host(0)).unwrap();
    let report = legacy
        .process_input(
            input(20_000_000, 9, 7, 0, ButtonState::Down),
            &mapper,
            raw(50_000_000),
        )
        .unwrap();
    assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
    assert_eq!(legacy.score().hits, 1);
}

#[test]
fn generated_cursor_and_bad_dispatch_arguments_cannot_advance_or_consume_input() {
    let mut step = solo(false);
    let mut merger = merger(vec![DeviceId(9)]);
    let original = input(10_000_000, 9, 7, 0, ButtonState::Down);
    merger.admit(original.clone(), host(20_000_000)).unwrap();
    step.record_audio_prefix(host(20_000_000)).unwrap();
    step.feed_audio(1_000_000, 8).unwrap();
    assert_eq!(step.song_time(), Timestamp::ZERO);
    assert_eq!(step.score().hits, 0);
    assert_eq!(step.score().misses, 0);
    assert!(
        step.advance_audio_frontier(&mut merger, host(20_000_000), raw(1_000_000_000))
            .unwrap()
            .is_none()
    );
    pair(&mut step);
    let before = state(&step);
    assert!(
        step.process_next_audio_input(&mut merger, host(20_000_000), host(20_000_000), None)
            .is_err()
    );
    assert_eq!(state(&step), before);
    assert!(
        step.process_next_audio_input(
            &mut merger,
            host(20_000_000),
            raw(50_000_000),
            Some(Position2 {
                x: f32::NAN,
                y: 0.0
            })
        )
        .is_err()
    );
    assert_eq!(state(&step), before);
    assert_eq!(
        merger.peek_ready(host(20_000_000)).unwrap(),
        Some(&original)
    );
}

#[test]
fn seeded_audio_constructor_preserves_logical_preroll_anchor_and_refuses_mismatched_setup() {
    let mut seeded = authority(false);
    seeded
        .observe(
            1,
            ClockPair {
                source: raw(0),
                target: host(0),
            },
        )
        .unwrap();
    seeded
        .observe(
            1,
            ClockPair {
                source: raw(40_000_000),
                target: host(20_000_000),
            },
        )
        .unwrap();
    let mut chosen = config();
    chosen.preroll = Duration::from_nanos(10_000_000);
    let (mut step, _) = StepGameplay::new_audio_section(
        prepared(),
        chosen,
        bindings(None),
        Timestamp::ZERO,
        None,
        BmsInputMode::ButtonOnly,
        seeded,
    )
    .unwrap();
    assert_eq!(step.song_time(), Timestamp::from_nanos(-10_000_000));
    let anchor = step.runtime.transport_mut().anchor();
    assert_eq!(anchor.host_time, logical(0).timestamp);
    assert_eq!(anchor.song_time, Timestamp::from_nanos(-10_000_000));
    assert_eq!(anchor.rate, Rate::NORMAL);
    let mut merger = merger(vec![DeviceId(9)]);
    let original = input(15_000_000, 9, 7, 0, ButtonState::Down);
    merger.admit(original.clone(), host(20_000_000)).unwrap();
    let before = state(&step);
    assert!(
        step.process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
            .is_err()
    );
    assert_eq!(state(&step), before);
    assert_eq!(merger.pending(), 1);
    step.activate_audio().unwrap();
    assert_eq!(step.runtime.transport_mut().anchor(), anchor);
    step.record_audio_prefix(host(20_000_000)).unwrap();
    let report = step
        .process_next_audio_input(&mut merger, host(20_000_000), raw(50_000_000), None)
        .unwrap()
        .unwrap();
    // HOST15ms -> logical+30ms; song+20ms after the actual 10ms preroll.
    assert_eq!(report.song_time, Timestamp::from_nanos(20_000_000));
    assert_eq!(step.score().hits, 1);
    assert_eq!(
        report.input.unwrap().meta().original_clock_point,
        Some(host(15_000_000))
    );
    let mut wrong = config();
    wrong.host_origin = point(44, HOST_ORIGIN);
    assert!(
        StepGameplay::new_audio_section(
            prepared(),
            wrong,
            bindings(None),
            Timestamp::ZERO,
            None,
            BmsInputMode::ButtonOnly,
            authority(false)
        )
        .is_err()
    );
    let mut wrong = config();
    wrong.output_origin = raw(1);
    assert!(
        StepGameplay::new_audio_section(
            prepared(),
            wrong,
            bindings(None),
            Timestamp::ZERO,
            None,
            BmsInputMode::ButtonOnly,
            authority(false)
        )
        .is_err()
    );
    let mut used = authority(false);
    used.observe(
        1,
        ClockPair {
            source: raw(0),
            target: host(0),
        },
    )
    .unwrap();
    used.observe(
        1,
        ClockPair {
            source: raw(40_000_000),
            target: host(20_000_000),
        },
    )
    .unwrap();
    used.record_acquired_prefix(host(20_000_000)).unwrap();
    let prepared_input = used
        .prepare_input(host(10_000_000), host(20_000_000))
        .unwrap()
        .unwrap();
    used.commit_input(prepared_input).unwrap();
    assert!(
        StepGameplay::new_audio_section(
            prepared(),
            config(),
            bindings(None),
            Timestamp::ZERO,
            None,
            BmsInputMode::ButtonOnly,
            used
        )
        .is_err()
    );
}

#[test]
fn local_audio_owner_rejects_all_generic_clock_and_judging_paths_without_mutation() {
    let mut step = local();
    let before = state(&step.control);
    let mapper = AffineClockMapper::exact_offset(
        ClockPair {
            source: host(0),
            target: raw(0),
        },
        ClockInterval {
            start: host(0).timestamp,
            end: host(1_000_000_000).timestamp,
        },
    )
    .unwrap();
    assert!(step.activate(host(0)).is_err());
    assert_eq!(state(&step.control), before);
    assert!(
        step.configure_output_clock(DisciplineConfig::default())
            .is_err()
    );
    assert_eq!(state(&step.control), before);
    assert!(
        step.observe_output_clock(ClockPair {
            source: raw(0),
            target: host(0)
        })
        .is_err()
    );
    assert_eq!(state(&step.control), before);
    assert!(step.update_output_clock(host(0)).is_err());
    assert_eq!(state(&step.control), before);
    assert!(
        step.process_input(
            input(20_000_000, 9, 7, 0, ButtonState::Down),
            &mapper,
            raw(50_000_000)
        )
        .is_err()
    );
    assert_eq!(state(&step.control), before);
    assert!(
        step.process_input_at(
            input(20_000_000, 9, 7, 0, ButtonState::Down),
            Position2 { x: 1.0, y: 1.0 },
            &mapper,
            raw(50_000_000)
        )
        .is_err()
    );
    assert_eq!(state(&step.control), before);
    assert!(
        step.advance_to(host(40_000_000), &mapper, raw(50_000_000))
            .is_err()
    );
    assert_eq!(state(&step.control), before);
    for id in [PlayerId(7), PlayerId(u32::MAX)] {
        assert_eq!(step.score(id).unwrap().hits, 0);
        assert_eq!(step.score(id).unwrap().misses, 0);
        assert_eq!(step.judge(id).unwrap().effective_song_time(), None);
    }
}
