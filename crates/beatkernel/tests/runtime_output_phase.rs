//! Phase-selected song timing runs through the existing physical Runtime owner.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, SourcePosition,
        VoiceId,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    runtime::{Runtime, RuntimeProcessingClock, RuntimeReport, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{OutputPhaseBasis, OutputPhasePoint, Rate, Transport},
};
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn basis(hz: u32, generation: u64) -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(1), 17, generation, 2, hz).unwrap()
}
fn phase(b: OutputPhaseBasis, host: i64, frame: u64) -> OutputPhasePoint {
    b.point(
        point(1, host),
        SourcePosition {
            frame,
            numerator: 0,
            denominator: 1,
        },
    )
    .unwrap()
}
fn phase_transport(b: OutputPhaseBasis, rate: Rate, end: Option<i64>) -> Transport {
    Transport::new_with_output_phase(b, point(1, 1000), ts(0), Duration::ZERO, rate, end.map(ts))
        .unwrap()
}
struct Mapper;
impl ClockMapper for Mapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == ClockDomainId(9) && to == ClockDomainId(1))
            .then(|| from.timestamp.checked_add(Duration::from_nanos(1000)))
            .flatten()
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn input(host: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(8), point(1, host), sequence),
        control: PhysicalControlId::keyboard(4),
        state,
    })
}
fn engine(note: Option<i64>, offset: i64, hazard: bool) -> JudgeEngine {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    if let Some(at) = note {
        chart.objects.push(SourceObject {
            id: ObjectId(1),
            start: Beat::new(at).unwrap(),
            end: None,
            interaction: InteractionId(1),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let mut judge = JudgeEngine::new(
        chart.compile().unwrap(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(PressInstantEvaluator),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::from_nanos(offset),
        )
        .unwrap(),
    )
    .unwrap();
    if hazard {
        judge
            .configure_hazards(
                HazardTimeline::new(
                    vec![HazardMarker {
                        id: HazardId(5),
                        at: ts(note.unwrap()),
                        control: GameControlId(1),
                        value: 9,
                    }],
                    1,
                )
                .unwrap(),
            )
            .unwrap();
    }
    judge
}
fn runtime(
    transport: Transport,
    note: Option<i64>,
    offset: i64,
    capacity: usize,
    sounds: usize,
    hazard: bool,
) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings([Binding {
        device: DeviceSelector::Exact(DeviceId(8)),
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(1),
    }])
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let sounds = (0..sounds)
        .map(|i| SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(i as u64 + 1),
            gain: 0.25,
        })
        .collect();
    let mut r = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport,
        bindings,
        engine(note, offset, hazard),
        producer,
        sounds,
        8,
    )
    .unwrap();
    r.set_processing_clock(RuntimeProcessingClock::Disabled);
    (r, consumer)
}
fn assert_report_eq(a: &RuntimeReport, b: &RuntimeReport) {
    assert_eq!(a.input, b.input);
    assert_eq!(a.bound_inputs, b.bound_inputs);
    assert_eq!(a.song_time, b.song_time);
    assert_eq!(a.song_end_reached, b.song_end_reached);
    assert_eq!(a.audio_at, b.audio_at);
    assert_eq!(a.input_mapping_quality, b.input_mapping_quality);
    assert_eq!(a.audio_mapping_quality, b.audio_mapping_quality);
    assert_eq!(a.judge_events, b.judge_events);
    assert_eq!(a.hazard_events, b.hazard_events);
    assert_eq!(a.judge_error, b.judge_error);
    assert_eq!(a.audio_commands, b.audio_commands);
    assert_eq!(a.audio_failures, b.audio_failures);
}

#[test]
fn four_rates_judge_literal_original_song_and_keep_physical_metadata_and_raw_keysound() {
    for (rate, song) in [
        (Rate::new(1, 2).unwrap(), 11_337),
        (Rate::NORMAL, 22_675),
        (Rate::new(2, 1).unwrap(), 45_350),
        (Rate::new(3, 2).unwrap(), 34_012),
    ] {
        let b = basis(44_100, 3);
        let (mut r, mut consumer) =
            runtime(phase_transport(b, rate, None), Some(song), 0, 4, 1, true);
        let mut physical = input(1100, 9, ButtonState::Down);
        physical.meta_mut().clock_domain = ClockDomainId(9);
        physical.meta_mut().timestamp = ts(100);
        physical.meta_mut().native = Some(NativeEventMeta {
            backend: BackendId(17),
            code: Some(4),
            timestamp: Some(point(9, 100)),
        });
        let original_native = physical.meta().native;
        let result = r
            .process_input_report_output_phase(physical, &Mapper, point(2, 777), phase(b, 1100, 3))
            .unwrap();
        let report = result.report;
        assert_eq!(report.song_time, ts(song));
        assert_eq!(report.audio_at, point(2, 777));
        let meta = report.input.as_ref().unwrap().meta();
        assert_eq!(meta.timestamp, ts(1100));
        assert_eq!(meta.clock_domain, ClockDomainId(1));
        assert_eq!(meta.original_clock_point, Some(point(9, 100)));
        assert_eq!(meta.native, original_native);
        assert_eq!(meta.sequence, 9);
        assert_eq!(meta.source, DeviceId(8));
        assert_eq!(report.bound_inputs.len(), 1);
        assert_eq!(report.bound_inputs[0].game_control, GameControlId(1));
        assert_eq!(report.judge_events.len(), 1);
        assert!(matches!(
            report.judge_events[0].outcome,
            JudgeOutcome::Hit { .. }
        ));
        assert_eq!(report.judge_events[0].at, ts(song));
        assert_eq!(report.judge_events[0].input, Some(*meta));
        assert_eq!(result.dispositions.len(), 1);
        assert_eq!(result.dispositions[0].selected(), Some(ObjectId(1)));
        assert!(result.refused.is_none());
        assert!(report.judge_error.is_none());
        assert_eq!(report.hazard_events.len(), 1);
        assert_eq!(report.hazard_events[0].outcome, HazardOutcome::Triggered);
        assert_eq!(report.hazard_events[0].input, Some(*meta));
        let expected = AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ts(777),
            gain: 0.25,
        };
        assert_eq!(report.audio_commands, [expected]);
        assert!(report.audio_failures.is_empty());
        assert_eq!(consumer.try_pop().unwrap(), expected);
        assert!(consumer.try_pop().is_err());
        let counters = r.telemetry().counters();
        assert_eq!(counters.inputs, 1);
        assert_eq!(counters.audio_commands, 1);
        assert_eq!(counters.judge_results, 1);
    }
}

#[test]
fn hz_three_one_frame_hits_499999999_without_absolute_timestamp_subtraction() {
    let b = basis(3, 3);
    let (mut r, _) = runtime(
        phase_transport(b, Rate::new(3, 2).unwrap(), None),
        Some(499_999_999),
        0,
        4,
        0,
        false,
    );
    let report = r
        .process_input_output_phase(
            input(1_000_000_000, 1, ButtonState::Down),
            &Mapper,
            point(2, 33),
            phase(b, 1_000_000_000, 3),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(499_999_999));
    assert_eq!(report.judge_events.len(), 1);
    assert!(matches!(
        report.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
}

#[test]
fn configured_profile_offset_is_applied_once_after_phase_mapping() {
    let b = basis(44_100, 3);
    let (mut r, _) = runtime(
        phase_transport(b, Rate::NORMAL, None),
        Some(22_680),
        5,
        4,
        0,
        false,
    );
    let report = r
        .process_input_output_phase(
            input(1100, 1, ButtonState::Down),
            &Mapper,
            point(2, 0),
            phase(b, 1100, 3),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(22_675));
    assert_eq!(report.judge_events.len(), 1);
    assert!(matches!(
        report.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
}

#[test]
fn wrong_phase_logical_point_and_basis_refuse_before_acquisition_or_judge_effects() {
    let b = basis(44_100, 3);
    let (mut r, mut queue) = runtime(
        phase_transport(b, Rate::NORMAL, None),
        Some(22_675),
        0,
        4,
        1,
        false,
    );
    let before = r.judge().stable_hash().unwrap();
    for q in [phase(b, 1099, 3), phase(basis(44_100, 4), 1100, 3)] {
        assert!(r
            .process_input_report_output_phase(
                input(1100, 10, ButtonState::Down),
                &Mapper,
                point(2, 7),
                q
            )
            .is_err());
        assert_eq!(r.judge().stable_hash().unwrap(), before);
        assert_eq!(r.telemetry().counters().inputs, 0);
        assert_eq!(r.telemetry().counters().audio_commands, 0);
        assert!(queue.try_pop().is_err());
    }
    let report = r
        .process_input_report_output_phase(
            input(1100, 1, ButtonState::Down),
            &Mapper,
            point(2, 7),
            phase(b, 1100, 3),
        )
        .unwrap();
    assert_eq!(
        report.dispositions[0].freshness(),
        InputFreshness::FreshPress
    );
}

#[test]
fn scalar_apis_cannot_process_phase_bound_runtime_or_touch_owner() {
    let b = basis(44_100, 3);
    let (mut r, mut queue) = runtime(
        phase_transport(b, Rate::NORMAL, None),
        Some(22_675),
        0,
        4,
        1,
        false,
    );
    r.configure_touch_router(
        TouchRouter::new(
            vec![TouchRegion {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(1),
                min: Position2 { x: 0.0, y: 0.0 },
                max: Position2 { x: 10.0, y: 10.0 },
            }],
            2,
        )
        .unwrap(),
    )
    .unwrap();
    let touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(8), point(1, 1100), 10),
        control: PhysicalControlId::keyboard(4),
        contact: ContactId(9),
        phase: TouchPhase::Down,
        position: Position2 { x: 2.0, y: 2.0 },
        pressure: None,
    });
    let before = r.judge().stable_hash().unwrap();
    assert!(r
        .process_input(input(1100, 10, ButtonState::Down), &Mapper, point(2, 7))
        .is_err());
    assert!(r
        .process_input_report(input(1100, 10, ButtonState::Down), &Mapper, point(2, 7))
        .is_err());
    assert!(r
        .process_input_at(
            touch.clone(),
            Position2 { x: 2.0, y: 2.0 },
            &Mapper,
            point(2, 7)
        )
        .is_err());
    assert!(r
        .process_input_at_report(
            touch.clone(),
            Position2 { x: 2.0, y: 2.0 },
            &Mapper,
            point(2, 7)
        )
        .is_err());
    assert!(r.advance_to(point(1, 1100), &Mapper, point(2, 7)).is_err());
    assert_eq!(r.judge().stable_hash().unwrap(), before);
    assert_eq!(r.touch_router().unwrap().active_contacts(), 0);
    assert_eq!(r.telemetry().counters().inputs, 0);
    assert_eq!(r.telemetry().counters().audio_commands, 0);
    assert!(queue.try_pop().is_err());
    let report = r
        .process_input_at_report_output_phase(
            touch,
            Position2 { x: 2.0, y: 2.0 },
            &Mapper,
            point(2, 7),
            phase(b, 1100, 3),
        )
        .unwrap();
    assert_eq!(r.touch_router().unwrap().active_contacts(), 1);
    assert_eq!(report.dispositions.len(), 1);
    assert_eq!(report.report.judge_events.len(), 1);
}

#[test]
fn regressing_elapsed_cannot_hide_in_equal_rate_rounded_song() {
    let b = basis(1_000_000_000, 3);
    let (mut r, _) = runtime(
        phase_transport(b, Rate::new(1, 2).unwrap(), None),
        None,
        0,
        4,
        0,
        false,
    );
    assert_eq!(
        r.advance_to_output_phase(point(1, 1100), &Mapper, point(2, 7), phase(b, 1100, 5))
            .unwrap()
            .song_time,
        ts(1)
    );
    let before = r.judge().stable_hash().unwrap();
    assert!(r
        .advance_to_output_phase(point(1, 1101), &Mapper, point(2, 8), phase(b, 1101, 4))
        .is_err());
    assert_eq!(r.judge().stable_hash().unwrap(), before);
    assert_eq!(
        r.advance_to_output_phase(point(1, 1101), &Mapper, point(2, 8), phase(b, 1101, 5))
            .unwrap()
            .song_time,
        ts(1)
    );
}

#[test]
fn finite_cap_and_gameplay_fence_do_not_mask_elapsed_regression() {
    for fence in [false, true] {
        let b = basis(1_000_000_000, 3);
        let (mut r, _) = runtime(
            phase_transport(b, Rate::NORMAL, Some(1)),
            None,
            0,
            4,
            0,
            false,
        );
        if !fence {
            r.set_song_end(ts(1)).unwrap();
        }
        r.advance_to_output_phase(point(1, 1100), &Mapper, point(2, 7), phase(b, 1100, 12))
            .unwrap();
        if fence {
            assert_eq!(r.fence_gameplay(), Some(ts(1)));
        }
        let before = r.judge().stable_hash().unwrap();
        assert!(r
            .process_input_output_phase(
                input(1101, 9, ButtonState::Down),
                &Mapper,
                point(2, 8),
                phase(b, 1101, 11)
            )
            .is_err());
        assert_eq!(r.judge().stable_hash().unwrap(), before);
        assert_eq!(r.telemetry().counters().inputs, 0);
        assert_eq!(
            r.process_input_output_phase(
                input(1101, 1, ButtonState::Down),
                &Mapper,
                point(2, 8),
                phase(b, 1101, 12)
            )
            .unwrap()
            .song_time,
            ts(1)
        );
    }
}

#[test]
fn acknowledged_transport_controls_bound_runtime_elapsed_before_effects() {
    let b = basis(1_000_000_000, 3);
    let (mut r, _) = runtime(
        phase_transport(b, Rate::new(1, 2).unwrap(), None),
        None,
        0,
        4,
        0,
        false,
    );
    r.transport_mut()
        .pause_at_output_phase(phase(b, 1100, 5))
        .unwrap();
    r.transport_mut()
        .resume_at_output_phase(phase(b, 1200, 5))
        .unwrap();
    assert!(r
        .advance_to_output_phase(point(1, 1201), &Mapper, point(2, 7), phase(b, 1201, 4))
        .is_err());
    assert_eq!(
        r.advance_to_output_phase(point(1, 1201), &Mapper, point(2, 7), phase(b, 1201, 5))
            .unwrap()
            .song_time,
        ts(1)
    );
}

#[test]
fn paired_replacement_clears_elapsed_watermark_and_refuses_stale_generation() {
    let old = basis(1_000_000_000, 3);
    let new = basis(1_000_000_000, 4);
    let (mut r, _) = runtime(
        phase_transport(old, Rate::NORMAL, None),
        None,
        0,
        4,
        0,
        false,
    );
    r.advance_to_output_phase(point(1, 1100), &Mapper, point(2, 7), phase(old, 1100, 100))
        .unwrap();
    r.replace_state(
        engine(Some(1), 0, false),
        phase_transport(new, Rate::NORMAL, None),
    );
    assert!(r
        .process_input_output_phase(
            input(1100, 9, ButtonState::Down),
            &Mapper,
            point(2, 8),
            phase(old, 1100, 3)
        )
        .is_err());
    let report = r
        .process_input_output_phase(
            input(1100, 1, ButtonState::Down),
            &Mapper,
            point(2, 8),
            phase(new, 1100, 3),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(1));
    assert_eq!(report.judge_events.len(), 1);
}

#[test]
fn queue_pressure_preserves_judged_hit_and_exact_admitted_failed_credit() {
    let b = basis(44_100, 3);
    let (mut r, mut queue) = runtime(
        phase_transport(b, Rate::NORMAL, None),
        Some(22_675),
        0,
        1,
        2,
        false,
    );
    let result = r
        .process_input_report_output_phase(
            input(1100, 1, ButtonState::Down),
            &Mapper,
            point(2, 777),
            phase(b, 1100, 3),
        )
        .unwrap();
    assert_eq!(result.report.judge_events.len(), 1);
    assert_eq!(result.dispositions.len(), 1);
    assert_eq!(result.report.audio_commands.len(), 1);
    assert_eq!(result.report.audio_failures.len(), 1);
    assert_eq!(result.report.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(
        result.report.audio_commands[0],
        AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: ts(777),
            gain: 0.25
        }
    );
    assert_eq!(
        result.report.audio_failures[0].command,
        AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: ts(777),
            gain: 0.25
        }
    );
    assert_eq!(queue.try_pop().unwrap(), result.report.audio_commands[0]);
    assert!(queue.try_pop().is_err());
    let c = r.telemetry().counters();
    assert_eq!(
        (c.inputs, c.judge_results, c.audio_commands, c.queue_full),
        (1, 1, 1, 1)
    );
}

#[test]
fn normal_integer_grid_phase_is_identical_to_legacy_reports_and_queue() {
    let b = basis(1_000_000_000, 3);
    let (mut phased, mut phased_queue) = runtime(
        phase_transport(b, Rate::NORMAL, None),
        Some(100),
        0,
        4,
        1,
        true,
    );
    let (mut legacy, mut legacy_queue) = runtime(
        Transport::new(ts(1000), ts(0), Rate::NORMAL),
        Some(100),
        0,
        4,
        1,
        true,
    );
    let a = phased
        .process_input_report_output_phase(
            input(1100, 1, ButtonState::Down),
            &Mapper,
            point(2, 777),
            phase(b, 1100, 102),
        )
        .unwrap();
    let z = legacy
        .process_input_report(input(1100, 1, ButtonState::Down), &Mapper, point(2, 777))
        .unwrap();
    assert_report_eq(&a.report, &z.report);
    assert_eq!(a.dispositions, z.dispositions);
    assert_eq!(a.refused, z.refused);
    assert_eq!(
        phased.judge().stable_hash().unwrap(),
        legacy.judge().stable_hash().unwrap()
    );
    assert_eq!(phased.telemetry().counters(), legacy.telemetry().counters());
    assert_eq!(
        phased_queue.try_pop().unwrap(),
        legacy_queue.try_pop().unwrap()
    );
    let a = phased
        .advance_to_output_phase(point(1, 1200), &Mapper, point(2, 888), phase(b, 1200, 202))
        .unwrap();
    let z = legacy
        .advance_to(point(1, 1200), &Mapper, point(2, 888))
        .unwrap();
    assert_report_eq(&a, &z);
}

#[test]
fn fractional_source_positions_produce_capture_ready_original_song_reports() {
    for (hz, numerator, denominator, song) in [(44_100, 121, 160, 17_148), (48_000, 26, 147, 3_684)]
    {
        let b = basis(hz, 3);
        let (mut r, _) = runtime(
            phase_transport(b, Rate::NORMAL, None),
            Some(song),
            0,
            4,
            0,
            false,
        );
        let q = b
            .point(
                point(1, 1100),
                SourcePosition {
                    frame: 2,
                    numerator,
                    denominator,
                },
            )
            .unwrap();
        let physical = input(1100, 1, ButtonState::Down);
        let report = r
            .process_input_output_phase(physical.clone(), &Mapper, point(2, 777), q)
            .unwrap();
        assert_eq!(report.song_time, ts(song));
        assert_eq!(report.input, Some(physical));
        assert_eq!(report.audio_at, point(2, 777));
        assert_eq!(report.judge_events.len(), 1);
        assert!(matches!(
            report.judge_events[0].outcome,
            JudgeOutcome::Hit { .. }
        ));
        // Existing recorder can persist this mapped song time alongside unchanged acquisition.
        // This pure core test does not claim app ReplayRecorder roundtrip verification.
    }
}

#[test]
fn phase_inputs_retain_native_chronology_sequence_and_output_domain_guards() {
    let b = basis(1_000_000_000, 3);
    let (mut r, _) = runtime(phase_transport(b, Rate::NORMAL, None), None, 0, 4, 0, false);
    r.process_input_output_phase(
        input(1100, 5, ButtonState::Down),
        &Mapper,
        point(2, 777),
        phase(b, 1100, 3),
    )
    .unwrap();
    let before = r.judge().stable_hash().unwrap();
    assert!(r
        .process_input_output_phase(
            input(1099, 6, ButtonState::Up),
            &Mapper,
            point(2, 778),
            phase(b, 1099, 4)
        )
        .is_err());
    assert!(r
        .process_input_output_phase(
            input(1101, 4, ButtonState::Up),
            &Mapper,
            point(2, 778),
            phase(b, 1101, 4)
        )
        .is_err());
    assert!(r
        .process_input_output_phase(
            input(1101, 6, ButtonState::Up),
            &Mapper,
            point(99, 778),
            phase(b, 1101, 4)
        )
        .is_err());
    assert_eq!(r.judge().stable_hash().unwrap(), before);
    assert_eq!(r.telemetry().counters().inputs, 1);
    let report = r
        .process_input_report_output_phase(
            input(1101, 6, ButtonState::Up),
            &Mapper,
            point(2, 778),
            phase(b, 1101, 4),
        )
        .unwrap();
    assert_eq!(report.report.song_time, ts(2));
    assert_eq!(report.dispositions[0].freshness(), InputFreshness::Other);
    assert_eq!(r.telemetry().counters().inputs, 2);
}

#[test]
fn positioned_phase_input_uses_projection_without_rewriting_physical_payload() {
    let b = basis(44_100, 3);
    let (mut r, _) = runtime(
        phase_transport(b, Rate::NORMAL, None),
        Some(22_675),
        0,
        4,
        0,
        false,
    );
    r.configure_touch_router(
        TouchRouter::new(
            vec![TouchRegion {
                device: DeviceSelector::Any,
                physical: PhysicalControlId::keyboard(4),
                game_control: GameControlId(1),
                min: Position2 { x: 0.0, y: 0.0 },
                max: Position2 { x: 10.0, y: 10.0 },
            }],
            2,
        )
        .unwrap(),
    )
    .unwrap();
    let touch = PhysicalInputEvent::Touch(TouchEvent {
        meta: EventMeta::new(DeviceId(8), point(1, 1100), 1),
        control: PhysicalControlId::keyboard(4),
        contact: ContactId(9),
        phase: TouchPhase::Down,
        position: Position2 {
            x: 500.0,
            y: -500.0,
        },
        pressure: Some(0.75),
    });
    let before = r.judge().stable_hash().unwrap();
    assert!(r
        .process_input_at_output_phase(
            touch.clone(),
            Position2 {
                x: f32::NAN,
                y: 2.0
            },
            &Mapper,
            point(2, 777),
            phase(b, 1100, 3)
        )
        .is_err());
    assert_eq!(r.touch_router().unwrap().active_contacts(), 0);
    assert_eq!(r.judge().stable_hash().unwrap(), before);
    let report = r
        .process_input_at_output_phase(
            touch.clone(),
            Position2 { x: 2.0, y: 2.0 },
            &Mapper,
            point(2, 777),
            phase(b, 1100, 3),
        )
        .unwrap();
    assert_eq!(report.input, Some(touch.clone()));
    assert_eq!(report.bound_inputs[0].physical, touch);
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(r.touch_router().unwrap().active_contacts(), 1);
}

#[test]
fn direct_elapsed_input_report_preserves_normalized_provenance_and_raw_audio_point() {
    let b = basis(44_100, 3);
    let (mut r, mut queue) = runtime(
        phase_transport(b, Rate::new(3, 2).unwrap(), None),
        Some(34_012),
        0,
        4,
        1,
        false,
    );
    let mut physical = input(1100, 1, ButtonState::Down);
    physical.meta_mut().clock_domain = ClockDomainId(9);
    physical.meta_mut().timestamp = ts(100);
    let q = b.point_from_elapsed_nanos(point(1, 1100), 22_675).unwrap();
    let result = r
        .process_input_report_output_phase(physical, &Mapper, point(2, 777), q)
        .unwrap();
    assert_eq!(result.report.song_time, ts(34_012));
    assert_eq!(result.report.audio_at, point(2, 777));
    let meta = result.report.input.as_ref().unwrap().meta();
    assert_eq!(meta.timestamp, ts(1100));
    assert_eq!(meta.clock_domain, ClockDomainId(1));
    assert_eq!(meta.original_clock_point, Some(point(9, 100)));
    assert_eq!(result.dispositions.len(), 1);
    assert_eq!(result.dispositions[0].selected(), Some(ObjectId(1)));
    assert!(matches!(
        result.report.judge_events[0].outcome,
        JudgeOutcome::Hit { .. }
    ));
    let command = AudioCommand::Play {
        voice: VoiceId(1),
        sample: SampleId(1),
        at: ts(777),
        gain: 0.25,
    };
    assert_eq!(result.report.audio_commands, [command]);
    assert_eq!(queue.try_pop().unwrap(), command);
    assert!(queue.try_pop().is_err());
}
