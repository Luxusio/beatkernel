//! Deferred actual Runtime routing, commit boundaries and paired owner restoration.
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig, PcmLimits,
        PcmSample, QueuePushError, SampleBank, SampleId, VoiceId, command_queue,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::*,
    interaction::{InteractionState, PressHoldEvaluator, PressInstantEvaluator},
    judge::{JudgeEngine, JudgeError, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{Runtime, RuntimeError, RuntimeProcessingClock, SoundBinding},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport},
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
fn pos(x: f32) -> Position2 {
    Position2 { x, y: 2.0 }
}
fn surface(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(17),
        code,
    }
}
fn transport() -> Transport {
    Transport::new(ts(1000), Timestamp::ZERO, Rate::NORMAL)
}
fn mapper() -> AffineClockMapper {
    AffineClockMapper::exact_offset(
        ClockPair {
            source: point(9, 0),
            target: point(1, 1000),
        },
        ClockInterval {
            start: Timestamp::ZERO,
            end: ts(10_000),
        },
    )
    .unwrap()
}
fn judge() -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = [(1, 100, Some(200), 1), (2, 100, None, 2), (3, 150, None, 2)]
        .map(|(id, start, end, rule)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(start).unwrap(),
            end: end.map(|end| Beat::new(end).unwrap()),
            interaction: InteractionId(rule),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .to_vec();
    JudgeEngine::new(
        source.compile().unwrap(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(PressHoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(2),
                evaluator: Box::new(PressInstantEvaluator),
            },
        ],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}
fn routing(cap: usize) -> TouchRouter {
    TouchRouter::new(
        [1, 2]
            .map(|lane| TouchRegion {
                device: DeviceSelector::Any,
                physical: surface(1),
                game_control: GameControlId(lane),
                min: Position2 {
                    x: (lane - 1) as f32 * 10.0,
                    y: 0.0,
                },
                max: Position2 {
                    x: lane as f32 * 10.0,
                    y: 10.0,
                },
            })
            .to_vec(),
        cap,
    )
    .unwrap()
}
fn sounds() -> Vec<SoundBinding> {
    vec![SoundBinding {
        object: ObjectId(1),
        stage: JudgeStage::HoldHead,
        sample: SampleId(1),
        voice: VoiceId(1),
        gain: 1.0,
    }]
}
fn fixture_with_sounds(capacity: usize, sounds: Vec<SoundBinding>) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings([1, 9].into_iter().flat_map(|physical| {
        [1, 2].map(move |lane| Binding {
            device: DeviceSelector::Any,
            physical: surface(physical),
            game_control: GameControlId(lane),
        })
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(),
        bindings,
        judge(),
        producer,
        sounds,
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    (runtime, consumer)
}
fn fixture(capacity: usize) -> (Runtime, CommandConsumer) {
    fixture_with_sounds(capacity, sounds())
}
fn input(
    at: i64,
    sequence: u64,
    contact: u64,
    phase: TouchPhase,
    control: u32,
    x: f32,
) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(u64::MAX), point(1, 1000 + at), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(17),
        code: Some(control),
        timestamp: Some(point(99, -7)),
    });
    PhysicalInputEvent::Touch(TouchEvent {
        meta,
        control: surface(control),
        contact: ContactId(contact),
        phase,
        position: pos(x),
        pressure: Some(12.5),
    })
}

#[test]
fn actual_runtime_routes_one_normalized_contact_and_retains_default_or_unconfigured_fanout() {
    let (mut runtime, consumer) = fixture(8);
    runtime.configure_touch_router(routing(4)).unwrap();
    let mut original = input(100, u64::MAX, u64::MAX, TouchPhase::Down, 1, 960.0);
    original.meta_mut().clock_domain = ClockDomainId(9);
    original.meta_mut().timestamp = ts(100);
    let report = runtime
        .process_input_at(original.clone(), pos(2.0), &mapper(), point(2, 1_000_000))
        .unwrap();
    let mut normalized = original.clone();
    normalized.meta_mut().clock_domain = ClockDomainId(1);
    normalized.meta_mut().timestamp = ts(1100);
    normalized.meta_mut().original_clock_point = Some(point(9, 100));
    assert_eq!(report.input, Some(normalized.clone()));
    assert_eq!(
        report.bound_inputs,
        [GameInputEvent {
            game_control: GameControlId(1),
            physical: normalized.clone()
        }]
    );
    assert_eq!(report.song_time, ts(100));
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(report.judge_events[0].input, Some(*normalized.meta()));
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 1);
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 2).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, -0.5], limits).unwrap(),
    )
    .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut pcm = [0.0; 4];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.0, 0.25, -0.5, 0.0]);
    for (configured, control) in [(false, 1), (true, 9)] {
        let (mut runtime, _consumer) = fixture(8);
        if configured {
            runtime.configure_touch_router(routing(4)).unwrap();
        }
        let report = runtime
            .process_input(
                input(100, 1, 7, TouchPhase::Down, control, 2.0),
                &mapper(),
                point(2, 0),
            )
            .unwrap();
        assert_eq!(
            report
                .bound_inputs
                .iter()
                .map(|event| event.game_control.0)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(report.judge_events.len(), 2);
        assert!(
            runtime
                .touch_router()
                .is_none_or(|router| router.active_contacts() == 0)
        );
    }
    let (mut ignored, _consumer) = fixture(8);
    ignored.configure_touch_router(routing(4)).unwrap();
    let report = ignored
        .process_input(
            input(100, 1, 7, TouchPhase::Down, 1, 30.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert!(report.bound_inputs.is_empty() && report.judge_events.is_empty());
    assert_eq!(ignored.telemetry().counters().unbound, 1);
    assert_eq!(ignored.touch_router().unwrap().active_contacts(), 1);
}

#[test]
fn clock_sequence_and_router_refusals_preserve_judge_contacts_and_uncommitted_acquisition_time() {
    let (mut runtime, _consumer) = fixture(8);
    runtime.configure_touch_router(routing(4)).unwrap();
    runtime
        .process_input(
            input(100, 10, 7, TouchPhase::Down, 1, 2.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    let hash = runtime.judge().stable_hash().unwrap();
    let mut invalid = input(150, 20, 8, TouchPhase::Down, 1, 12.0);
    if let PhysicalInputEvent::Touch(event) = &mut invalid {
        event.pressure = Some(f32::NAN);
    }
    assert!(matches!(
        runtime.process_input(invalid, &mapper(), point(2, 0)),
        Err(RuntimeError::TouchRouting(
            TouchRoutingError::NonFiniteSample
        ))
    ));
    assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 1);
    assert_eq!(runtime.telemetry().counters().inputs, 1);
    assert_eq!(runtime.telemetry().counters().rejected, 1);
    // Earlier host/sequence than the rejected sample remain admissible.
    let report = runtime
        .process_input(
            input(120, 11, 8, TouchPhase::Down, 1, 12.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert_eq!(report.bound_inputs.len(), 1);
    let hash = runtime.judge().stable_hash().unwrap();
    let before = runtime.telemetry().counters();
    for (at, sequence, output, expected) in [
        (
            200,
            30,
            point(99, 0),
            RuntimeError::UnmappedClock {
                from: ClockDomainId(99),
                to: ClockDomainId(2),
            },
        ),
        (119, 40, point(2, 0), RuntimeError::NonMonotonicHost),
        (
            130,
            5,
            point(2, 0),
            RuntimeError::SequenceRegression {
                device: DeviceId(u64::MAX),
                last: 11,
                received: 5,
            },
        ),
    ] {
        let invalid = input(at, sequence, 7, TouchPhase::Up, 1, f32::NAN);
        assert_eq!(
            runtime
                .process_input(invalid, &mapper(), output)
                .unwrap_err(),
            expected
        );
        assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
        assert_eq!(runtime.touch_router().unwrap().active_contacts(), 2);
        assert_eq!(runtime.telemetry().counters().inputs, before.inputs);
    }
    let report = runtime
        .process_input(
            input(130, 12, 8, TouchPhase::Up, 1, 12.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(130));
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 1);
}

#[test]
fn capacity_and_projected_release_errors_do_not_consume_sequences_or_prevent_existing_release() {
    let (mut runtime, _consumer) = fixture(8);
    runtime.configure_touch_router(routing(1)).unwrap();
    assert!(matches!(
        runtime.configure_touch_router(routing(1)),
        Err(RuntimeError::TouchRoutingConfigurationLocked)
    ));
    runtime
        .process_input(
            input(90, 1, 8, TouchPhase::Down, 1, 30.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    let new = input(100, 2, 7, TouchPhase::Down, 1, 2.0);
    assert!(matches!(
        runtime.process_input(new.clone(), &mapper(), point(2, 0)),
        Err(RuntimeError::TouchRouting(
            TouchRoutingError::ContactCapacity
        ))
    ));
    let release = runtime
        .process_input(
            input(100, 2, 8, TouchPhase::Up, 1, 2.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert!(release.bound_inputs.is_empty());
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 0);
    assert_eq!(
        runtime
            .process_input(new, &mapper(), point(2, 0))
            .unwrap()
            .judge_events
            .len(),
        1
    );
    let up = input(200, 3, 7, TouchPhase::Up, 1, 1000.0);
    assert!(matches!(
        runtime.process_input_at(up.clone(), pos(f32::NAN), &mapper(), point(2, 0)),
        Err(RuntimeError::TouchRouting(
            TouchRoutingError::NonFiniteSample
        ))
    ));
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 1);
    let report = runtime
        .process_input_at(up, pos(-1.0), &mapper(), point(2, 0))
        .unwrap();
    assert!(
        report
            .judge_events
            .iter()
            .any(|event| event.object == ObjectId(1) && event.stage == JudgeStage::HoldTail)
    );
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 0);
    let (mut started, _consumer) = fixture(8);
    started
        .advance_to(point(1, 1000), &mapper(), point(2, 0))
        .unwrap();
    assert!(matches!(
        started.configure_touch_router(routing(1)),
        Err(RuntimeError::TouchRoutingConfigurationLocked)
    ));
    assert!(started.touch_router().is_none());
}

#[test]
fn finite_end_skips_contact_and_projection_admission_but_retains_acquisition_clock_validation() {
    let (mut runtime, _consumer) = fixture(8);
    runtime.configure_touch_router(routing(2)).unwrap();
    runtime.set_song_end(ts(150)).unwrap();
    runtime
        .process_input(
            input(100, 1, 7, TouchPhase::Down, 1, 2.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    for (at, sequence) in [(150, 2), (999, 3)] {
        let mut input = input(at, sequence, 8, TouchPhase::Down, 1, f32::NAN);
        if let PhysicalInputEvent::Touch(event) = &mut input {
            event.pressure = Some(f32::INFINITY);
        }
        let report = runtime
            .process_input_at(input, pos(f32::NAN), &mapper(), point(2, 0))
            .unwrap();
        assert!(report.song_end_reached);
        assert_eq!(report.song_time, ts(150));
        assert!(report.input.is_none() && report.bound_inputs.is_empty());
        assert_eq!(runtime.touch_router().unwrap().active_contacts(), 1);
    }
    assert_eq!(
        runtime.judge().state(ObjectId(1)),
        Some(InteractionState::Active)
    );
    assert_eq!(
        runtime.judge().state(ObjectId(3)),
        Some(InteractionState::Pending)
    );
    let mut foreign = input(1000, 4, 9, TouchPhase::Down, 1, 2.0);
    foreign.meta_mut().clock_domain = ClockDomainId(99);
    assert!(matches!(
        runtime.process_input(foreign, &mapper(), point(2, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert_eq!(runtime.telemetry().counters().inputs, 3);
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 1);
}

#[test]
fn reported_judge_and_audio_failures_keep_committed_routing_and_the_exact_audio_prefix() {
    let mut both = sounds();
    both.push(SoundBinding {
        object: ObjectId(1),
        stage: JudgeStage::HoldHead,
        sample: SampleId(1),
        voice: VoiceId(2),
        gain: 0.5,
    });
    let (mut runtime, mut consumer) = fixture_with_sounds(1, both);
    runtime.configure_touch_router(routing(2)).unwrap();
    let report = runtime
        .process_input(
            input(100, 1, 7, TouchPhase::Down, 1, 2.0),
            &mapper(),
            point(2, 700),
        )
        .unwrap();
    assert_eq!(
        (
            report.bound_inputs.len(),
            report.judge_events.len(),
            report.audio_commands.len(),
            report.audio_failures.len()
        ),
        (1, 1, 1, 1)
    );
    assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(
        report.audio_failures[0].command,
        AudioCommand::Play {
            voice: VoiceId(2),
            sample: SampleId(1),
            at: ts(700),
            gain: 0.5
        }
    );
    assert_eq!(consumer.try_pop().unwrap(), report.audio_commands[0]);
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 1);
    assert_eq!(
        runtime.judge().state(ObjectId(1)),
        Some(InteractionState::Active)
    );
    let duplicate = runtime
        .process_input(
            input(100, 2, 7, TouchPhase::Down, 1, 12.0),
            &mapper(),
            point(2, 701),
        )
        .unwrap();
    assert!(duplicate.judge_events.is_empty() && duplicate.audio_commands.is_empty());
    assert!(
        consumer.try_pop().is_err(),
        "the rejected suffix is not silently retried"
    );

    let (mut failed, _consumer) = fixture(8);
    failed.configure_touch_router(routing(2)).unwrap();
    failed.judge_mut().advance_to(ts(200)).unwrap();
    let hash = failed.judge().stable_hash().unwrap();
    let report = failed
        .process_input(
            input(100, 9, 7, TouchPhase::Down, 1, 2.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert_eq!(report.judge_error, Some(JudgeError::NonMonotonicSongTime));
    assert!(report.input.is_some() && report.bound_inputs.is_empty());
    assert_eq!(failed.judge().stable_hash().unwrap(), hash);
    assert_eq!(
        failed.touch_router().unwrap().active_contacts(),
        1,
        "routing and acquisition already committed before the judge's reported refusal"
    );
    assert_eq!(failed.telemetry().counters().inputs, 1);
    assert!(matches!(
        failed.process_input(
            input(101, 8, 7, TouchPhase::Up, 1, 2.0),
            &mapper(),
            point(2, 0)
        ),
        Err(RuntimeError::SequenceRegression {
            last: 9,
            received: 8,
            ..
        })
    ));
}

#[test]
fn explicit_router_clone_restores_paired_contact_ownership_while_legacy_replacement_is_fresh() {
    let (mut runtime, _consumer) = fixture(8);
    runtime.configure_touch_router(routing(3)).unwrap();
    runtime
        .process_input(
            input(100, 10, 7, TouchPhase::Down, 1, 2.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    runtime
        .process_input(
            input(105, 11, 8, TouchPhase::Down, 1, 30.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    let snapshot = runtime.judge().snapshot().unwrap();
    let saved_router = runtime.touch_router().unwrap().try_clone().unwrap();
    assert_eq!(saved_router.active_contacts(), 2);
    assert_eq!(saved_router.max_contacts(), 3);
    let mut independent = saved_router.try_clone().unwrap();
    assert!(matches!(
        independent
            .route(&input(105, 12, 9, TouchPhase::Down, 1, 12.0))
            .unwrap(),
        TouchRoute::Bound(_)
    ));
    assert_eq!(independent.active_contacts(), 3);
    assert_eq!(saved_router.active_contacts(), 2);
    runtime
        .process_input(
            input(200, 12, 7, TouchPhase::Up, 1, 12.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    runtime
        .process_input(
            input(200, 13, 8, TouchPhase::Cancel, 1, 2.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    let (old_judge, _, old_router) = runtime.replace_state_with_touch_router(
        JudgeEngine::from_snapshot(&snapshot).unwrap(),
        transport(),
        Some(saved_router),
    );
    assert_eq!(
        old_judge.state(ObjectId(1)),
        Some(InteractionState::Completed)
    );
    assert_eq!(old_router.unwrap().active_contacts(), 0);
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 2);
    let duplicate = runtime
        .process_input(
            input(100, 1, 7, TouchPhase::Down, 1, 12.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert!(duplicate.judge_events.is_empty());
    assert_eq!(duplicate.bound_inputs[0].game_control, GameControlId(1));
    let outside = runtime
        .process_input(
            input(100, 2, 8, TouchPhase::Down, 1, 2.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert!(
        outside.bound_inputs.is_empty(),
        "the restored outside contact remains None"
    );
    let released = runtime
        .process_input(
            input(200, 3, 7, TouchPhase::Up, 1, 12.0),
            &mapper(),
            point(2, 0),
        )
        .unwrap();
    assert!(
        released
            .judge_events
            .iter()
            .any(|event| event.stage == JudgeStage::HoldTail)
    );
    let regions = runtime.touch_router().unwrap().regions().to_vec();
    runtime.replace_state(judge(), transport());
    assert_eq!(runtime.touch_router().unwrap().active_contacts(), 0);
    assert_eq!(runtime.touch_router().unwrap().regions(), regions);
    assert_eq!(runtime.touch_router().unwrap().max_contacts(), 3);
    assert!(runtime.song_end().is_none());
    assert_eq!(
        runtime
            .process_input(
                input(100, 0, 7, TouchPhase::Down, 1, 2.0),
                &mapper(),
                point(2, 0)
            )
            .unwrap()
            .judge_events
            .len(),
        1
    );
    let (_, _, detached) = runtime.replace_state_with_touch_router(judge(), transport(), None);
    assert_eq!(detached.unwrap().active_contacts(), 1);
    assert!(runtime.touch_router().is_none());
    assert_eq!(
        runtime
            .process_input(
                input(100, 0, 7, TouchPhase::Down, 1, 2.0),
                &mapper(),
                point(2, 0)
            )
            .unwrap()
            .bound_inputs
            .len(),
        2
    );
}
