//! Deferred actual Runtime delivery fixtures. These exercise software owners,
//! not application mine plans, gauge/death policy or native audio devices.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    replay::{ReplayHeader, ReplayRecorder, ReplaySession, REPLAY_VERSION},
    runtime::{Runtime, RuntimeError, RuntimeProcessingClock, SoundBinding},
    time::{
        AffineClockMapper, ClockDomainId, ClockInterval, ClockMapper, ClockMappingQuality,
        ClockPair, ClockPoint, Duration, Timestamp,
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
struct NoMapping;
impl ClockMapper for NoMapping {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn physical(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(17),
        code,
    }
}
fn meta(song: i64, sequence: u64) -> EventMeta {
    let mut meta = EventMeta::new(DeviceId(u64::MAX), point(1, 1000 + song), sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(17),
        code: Some(u32::MAX),
        timestamp: Some(point(9, -7)),
    });
    meta
}
fn button(song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(song, sequence),
        control: physical(7),
        state,
    })
}
fn touch(song: i64, sequence: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(song, sequence),
        control: physical(7),
        contact: ContactId(u64::MAX),
        phase,
        position: Position2 {
            x: 900.5,
            y: -900.25,
        },
        pressure: Some(12.5),
    })
}
fn unbound(song: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(song, sequence),
        control: physical(99),
        state: ButtonState::Down,
    })
}
fn bound(input: PhysicalInputEvent, control: u32) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: input,
    }
}
fn marker(id: u64, at: i64, control: u32, value: u64) -> HazardMarker {
    HazardMarker {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(control),
        value,
    }
}
fn hazard(
    id: u64,
    at: i64,
    control: u32,
    value: u64,
    outcome: HazardOutcome,
    input: Option<EventMeta>,
) -> HazardEvent {
    HazardEvent {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(control),
        value,
        outcome,
        input,
    }
}
fn engine(
    notes: &[(u64, i64, u32)],
    offset: i64,
    resolver: Box<dyn CandidateResolver>,
    hazards: Option<Vec<HazardMarker>>,
) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = notes
        .iter()
        .map(|&(id, at, _)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(at).unwrap(),
            end: None,
            interaction: InteractionId(id as u32),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    let rules = notes
        .iter()
        .map(|&(id, _, control)| Rule {
            interaction: InteractionId(id as u32),
            control: GameControlId(control),
            evaluator: Box::new(PressInstantEvaluator),
        })
        .collect();
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(7),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::from_nanos(offset),
    )
    .unwrap();
    let mut judge = JudgeEngine::with_policies_and_contacts(
        source.compile().unwrap(),
        rules,
        profile,
        resolver,
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    if let Some(markers) = hazards {
        judge
            .configure_hazards(HazardTimeline::new(markers, 100).unwrap())
            .unwrap();
    }
    judge
}
fn transport() -> Transport {
    Transport::new(ts(1000), ts(0), Rate::NORMAL)
}
fn runtime(
    judge: JudgeEngine,
    controls: &[u32],
    capacity: usize,
    sounds: Vec<SoundBinding>,
) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings(controls.iter().map(|&control| Binding {
        device: DeviceSelector::Any,
        physical: physical(7),
        game_control: GameControlId(control),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(),
        bindings,
        judge,
        producer,
        sounds,
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    (runtime, consumer)
}
fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"runtime-hazards-fixture".to_vec(),
        rules_identity: b"actual-press/v1".to_vec(),
        options: vec![],
        seed: 0,
        normalized_clock: ClockDomainId(1),
    }
}

#[test]
fn first_fanout_call_consumes_all_boundary_markers_and_unbound_or_failed_acquisition_never_republishes_them()
 {
    for (controls, outcomes) in [
        (
            vec![1, 2],
            [HazardOutcome::Avoided, HazardOutcome::Triggered],
        ),
        (
            vec![2, 1],
            [HazardOutcome::Triggered, HazardOutcome::Avoided],
        ),
    ] {
        let judge = engine(
            &[],
            0,
            Box::new(ClosestCandidate),
            Some(vec![
                marker(u64::MAX, 10, 2, u64::MAX),
                marker(0, 10, 1, 0),
                marker(3, 20, 1, 3),
            ]),
        );
        let (mut owner, mut consumer) = runtime(judge, &controls, 4, vec![]);
        let input = button(10, 1, ButtonState::Down);
        let report = owner
            .process_input(input.clone(), &NoMapping, point(2, 700))
            .unwrap();
        assert_eq!(
            report.bound_inputs,
            controls
                .iter()
                .map(|&control| bound(input.clone(), control))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            report.hazard_events,
            [
                hazard(u64::MAX, 10, 2, u64::MAX, outcomes[0], Some(meta(10, 1))),
                hazard(0, 10, 1, 0, outcomes[1], Some(meta(10, 1)))
            ]
        );
        assert!(
            owner.judge().hazard_events().is_empty(),
            "the second successful call cleared its own report, not the Runtime prefix"
        );
        assert!(report.judge_events.is_empty() && report.judge_error.is_none());
        assert!(report.audio_commands.is_empty() && report.audio_failures.is_empty());
        assert_eq!(
            (
                owner.telemetry().counters().judge_results,
                owner.telemetry().counters().audio_commands
            ),
            (0, 0)
        );
        assert!(consumer.try_pop().is_err());
        let hash = owner.judge().stable_hash().unwrap();
        let ignored = owner
            .process_input(unbound(15, 2), &NoMapping, point(2, 701))
            .unwrap();
        assert!(
            ignored.input.is_some()
                && ignored.bound_inputs.is_empty()
                && ignored.hazard_events.is_empty()
        );
        assert_eq!(owner.judge().stable_hash().unwrap(), hash);
        assert!(matches!(
            owner.process_input(button(16, 1, ButtonState::Up), &NoMapping, point(2, 702)),
            Err(RuntimeError::SequenceRegression {
                last: 2,
                received: 1,
                ..
            })
        ));
        assert_eq!(
            owner
                .process_input(button(14, 3, ButtonState::Up), &NoMapping, point(2, 702))
                .unwrap_err(),
            RuntimeError::NonMonotonicHost
        );
        let mut unmapped = button(16, 3, ButtonState::Up);
        unmapped.meta_mut().clock_domain = ClockDomainId(99);
        assert!(matches!(
            owner.process_input(unmapped, &NoMapping, point(2, 702)),
            Err(RuntimeError::UnmappedClock { .. })
        ));
        assert!(matches!(
            owner.advance_to(point(1, 1016), &NoMapping, point(99, 702)),
            Err(RuntimeError::UnmappedClock { .. })
        ));
        assert_eq!(owner.judge().stable_hash().unwrap(), hash);
        let advanced = owner
            .advance_to(point(1, 1020), &NoMapping, point(2, 703))
            .unwrap();
        assert_eq!(
            advanced.hazard_events,
            [hazard(3, 20, 1, 3, HazardOutcome::Triggered, None)]
        );
        let retained = owner.judge().stable_hash().unwrap();
        let later_unbound = owner
            .process_input(unbound(21, 3), &NoMapping, point(2, 704))
            .unwrap();
        assert!(later_unbound.hazard_events.is_empty());
        assert_eq!(owner.judge().hazard_events(), advanced.hazard_events);
        assert_eq!(owner.judge().stable_hash().unwrap(), retained);
    }
    let (mut legacy, _) = runtime(
        engine(&[], 0, Box::new(ClosestCandidate), None),
        &[1, 2],
        1,
        vec![],
    );
    assert!(
        legacy
            .process_input(button(10, 1, ButtonState::Down), &NoMapping, point(2, 0))
            .unwrap()
            .hazard_events
            .is_empty()
    );
    assert!(
        legacy
            .advance_to(point(1, 1020), &NoMapping, point(2, 1))
            .unwrap()
            .hazard_events
            .is_empty()
    );
}

#[derive(Clone, Copy)]
struct RejectSecond;
impl CandidateResolver for RejectSecond {
    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        candidates.first().map(|candidate| {
            if candidate.object == ObjectId(2) {
                ObjectId(999)
            } else {
                candidate.object
            }
        })
    }
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        Some(b"runtime-hazard-reject-second/v1".to_vec())
    }
}

#[test]
fn later_resolver_failure_keeps_exact_successful_prefix_and_first_failure_does_not_copy_old_judge_report()
 {
    let make = || {
        engine(
            &[(1, 100, 1), (2, 100, 2)],
            0,
            Box::new(RejectSecond),
            Some(vec![
                marker(0, 0, 1, 0),
                marker(1, 100, 1, 1),
                marker(2, 100, 2, 2),
                marker(3, 100, 3, 3),
            ]),
        )
    };
    let (mut owner, mut consumer) = runtime(make(), &[1, 2, 3], 4, vec![]);
    owner
        .advance_to(point(1, 1000), &NoMapping, point(2, 0))
        .unwrap();
    let input = button(100, 1, ButtonState::Down);
    let report = owner
        .process_input(input.clone(), &NoMapping, point(2, 1))
        .unwrap();
    assert_eq!(
        report.judge_error,
        Some(JudgeError::InvalidCandidate {
            object: ObjectId(999)
        })
    );
    assert_eq!(report.bound_inputs, [bound(input.clone(), 1)]);
    assert_eq!(
        report.judge_events,
        [JudgeEvent {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            at: ts(100),
            input: Some(meta(100, 1)),
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(7),
                delta: Duration::ZERO
            }
        }]
    );
    assert_eq!(
        report.hazard_events,
        [
            hazard(1, 100, 1, 1, HazardOutcome::Triggered, Some(meta(100, 1))),
            hazard(2, 100, 2, 2, HazardOutcome::Avoided, Some(meta(100, 1))),
            hazard(3, 100, 3, 3, HazardOutcome::Avoided, Some(meta(100, 1)))
        ]
    );
    assert_eq!(owner.judge().hazard_events(), report.hazard_events);
    assert!(!owner.judge().is_fresh_press(&bound(input.clone(), 1)));
    assert!(owner.judge().is_fresh_press(&bound(input.clone(), 2)));
    assert!(owner.judge().is_fresh_press(&bound(input, 3)));
    assert_eq!(owner.telemetry().counters().judge_results, 1);
    assert!(
        report.audio_commands.is_empty()
            && report.audio_failures.is_empty()
            && consumer.try_pop().is_err()
    );
    let mut recorder = ReplayRecorder::new(header()).unwrap();
    recorder.record_report(&report).unwrap();
    assert_eq!(
        recorder.records().len(),
        1,
        "only the actual accepted bound prefix is recordable"
    );

    let (mut first_fails, _) = runtime(make(), &[2, 1], 4, vec![]);
    let old = first_fails
        .advance_to(point(1, 1000), &NoMapping, point(2, 0))
        .unwrap();
    assert_eq!(
        old.hazard_events,
        [hazard(0, 0, 1, 0, HazardOutcome::Avoided, None)]
    );
    let hash = first_fails.judge().stable_hash().unwrap();
    let failed = first_fails
        .process_input(button(100, 1, ButtonState::Down), &NoMapping, point(2, 1))
        .unwrap();
    assert_eq!(
        failed.judge_error,
        Some(JudgeError::InvalidCandidate {
            object: ObjectId(999)
        })
    );
    assert!(
        failed.bound_inputs.is_empty()
            && failed.judge_events.is_empty()
            && failed.hazard_events.is_empty()
    );
    assert_eq!(first_fails.judge().hazard_events(), old.hazard_events);
    assert_eq!(first_fails.judge().stable_hash().unwrap(), hash);
    let advance = first_fails
        .advance_to(point(1, 1100), &NoMapping, point(2, 2))
        .unwrap();
    assert_eq!(
        advance.hazard_events,
        [
            hazard(1, 100, 1, 1, HazardOutcome::Avoided, None),
            hazard(2, 100, 2, 2, HazardOutcome::Avoided, None),
            hazard(3, 100, 3, 3, HazardOutcome::Avoided, None)
        ]
    );
}

#[test]
fn mapped_projected_touch_uses_actual_router_ownership_preserves_provenance_and_leaves_ignored_markers_pending()
 {
    let judge = engine(
        &[],
        0,
        Box::new(ClosestCandidate),
        Some(vec![
            marker(1, 10, 1, 1),
            marker(2, 15, 1, 2),
            marker(3, 20, 1, 3),
            marker(4, 30, 1, 4),
            marker(5, 40, 1, 5),
        ]),
    );
    let (mut owner, mut consumer) = runtime(judge, &[1, 2], 4, vec![]);
    let regions = [(1, 0.0, 10.0), (2, 10.0, 20.0)]
        .into_iter()
        .map(|(control, min, max)| TouchRegion {
            device: DeviceSelector::Any,
            physical: physical(7),
            game_control: GameControlId(control),
            min: Position2 { x: min, y: 0.0 },
            max: Position2 { x: max, y: 10.0 },
        })
        .collect();
    owner
        .configure_touch_router(TouchRouter::new(regions, 2).unwrap())
        .unwrap();
    let mapper = AffineClockMapper::exact_offset(
        ClockPair {
            source: point(9, 0),
            target: point(1, 1000),
        },
        ClockInterval {
            start: ts(0),
            end: ts(100),
        },
    )
    .unwrap();
    for (index, (song, phase, projected, outcome)) in [
        (10, TouchPhase::Down, 5.0, HazardOutcome::Triggered),
        (15, TouchPhase::Move, 15.0, HazardOutcome::Triggered),
        (20, TouchPhase::Cancel, -50.0, HazardOutcome::Avoided),
    ]
    .into_iter()
    .enumerate()
    {
        let sequence = u64::MAX - 4 + index as u64;
        let mut original = touch(song, sequence, phase);
        original.meta_mut().clock_domain = ClockDomainId(9);
        original.meta_mut().timestamp = ts(song);
        let mut normalized = original.clone();
        normalized.meta_mut().clock_domain = ClockDomainId(1);
        normalized.meta_mut().timestamp = ts(1000 + song);
        normalized.meta_mut().original_clock_point = Some(point(9, song));
        if index == 1 {
            let before = owner.judge().stable_hash().unwrap();
            let retained = owner.judge().hazard_events().to_vec();
            assert!(matches!(
                owner.process_input_at(
                    original.clone(),
                    Position2 {
                        x: f32::NAN,
                        y: 5.0
                    },
                    &mapper,
                    point(2, 100)
                ),
                Err(RuntimeError::TouchRouting(_))
            ));
            assert_eq!(owner.judge().stable_hash().unwrap(), before);
            assert_eq!(owner.judge().hazard_events(), retained);
            assert_eq!(owner.touch_router().unwrap().active_contacts(), 1);
        }
        let report = owner
            .process_input_at(
                original.clone(),
                Position2 {
                    x: projected,
                    y: 5.0,
                },
                &mapper,
                point(2, 9_007_199_254_740_993 + index as i64),
            )
            .unwrap();
        assert_eq!(report.input, Some(normalized.clone()));
        assert_eq!(report.bound_inputs, [bound(normalized.clone(), 1)]);
        assert_eq!(
            report.hazard_events,
            [hazard(
                index as u64 + 1,
                song,
                1,
                index as u64 + 1,
                outcome,
                Some(*normalized.meta())
            )]
        );
        assert!(
            report.judge_events.is_empty()
                && report.judge_error.is_none()
                && report.audio_commands.is_empty()
        );
        let PhysicalInputEvent::Touch(payload) = report.input.unwrap() else {
            panic!("the original touch variant must survive")
        };
        assert_eq!(
            payload.position,
            Position2 {
                x: 900.5,
                y: -900.25
            }
        );
        assert_eq!(payload.contact, ContactId(u64::MAX));
        assert_eq!(payload.pressure, Some(12.5));
        assert_eq!(original.meta().clock_domain, ClockDomainId(9));
    }
    assert_eq!(owner.touch_router().unwrap().active_contacts(), 0);
    let retained = owner.judge().hazard_events().to_vec();
    for (song, sequence, phase, id) in [
        (30, u64::MAX - 1, TouchPhase::Down, 4),
        (40, u64::MAX, TouchPhase::Up, 5),
    ] {
        let ignored = owner
            .process_input_at(
                touch(song, sequence, phase),
                Position2 { x: 50.0, y: 5.0 },
                &NoMapping,
                point(2, song),
            )
            .unwrap();
        assert!(
            ignored.bound_inputs.is_empty()
                && ignored.hazard_events.is_empty()
                && ignored.judge_error.is_none()
        );
        if song == 30 {
            assert_eq!(owner.judge().hazard_events(), retained);
        }
        let advance = owner
            .advance_to(point(1, 1000 + song), &NoMapping, point(2, song + 1))
            .unwrap();
        assert_eq!(
            advance.hazard_events,
            [hazard(id, song, 1, id, HazardOutcome::Avoided, None)]
        );
    }
    assert_eq!(owner.telemetry().counters().unbound, 2);
    assert_eq!(owner.telemetry().counters().judge_results, 0);
    assert!(consumer.try_pop().is_err());
}

#[test]
fn finite_endpoint_advances_use_existing_owners_and_failed_advances_never_copy_the_retained_report()
{
    for held in [false, true] {
        let judge = engine(
            &[],
            0,
            Box::new(ClosestCandidate),
            Some(vec![
                marker(1, 10, 1, 1),
                marker(2, 15, 1, 2),
                marker(3, 20, 1, 3),
                marker(4, 21, 1, 4),
            ]),
        );
        let (mut owner, mut consumer) = runtime(judge, &[1], 4, vec![]);
        owner.set_song_end(ts(20)).unwrap();
        if held {
            owner
                .process_input(button(10, 1, ButtonState::Down), &NoMapping, point(2, 0))
                .unwrap();
        }
        let ending = owner
            .process_input(
                button(
                    20,
                    2,
                    if held {
                        ButtonState::Up
                    } else {
                        ButtonState::Down
                    },
                ),
                &NoMapping,
                point(2, 1),
            )
            .unwrap();
        assert!(
            ending.song_end_reached && ending.input.is_none() && ending.bound_inputs.is_empty()
        );
        let outcome = if held {
            HazardOutcome::Triggered
        } else {
            HazardOutcome::Avoided
        };
        let expected = if held {
            vec![
                hazard(2, 15, 1, 2, outcome, None),
                hazard(3, 20, 1, 3, outcome, None),
            ]
        } else {
            vec![
                hazard(1, 10, 1, 1, outcome, None),
                hazard(2, 15, 1, 2, outcome, None),
                hazard(3, 20, 1, 3, outcome, None),
            ]
        };
        assert_eq!(ending.hazard_events, expected);
        assert!(ending.judge_events.is_empty() && ending.audio_commands.is_empty());
        assert_eq!(
            owner
                .judge()
                .is_fresh_press(&bound(button(20, 2, ButtonState::Down), 1)),
            !held
        );
        let capped = owner
            .advance_to(point(1, 1100), &NoMapping, point(2, 2))
            .unwrap();
        assert!(capped.song_end_reached && capped.hazard_events.is_empty());
        assert_eq!(capped.song_time, ts(20));
        let state = owner.judge().snapshot().unwrap();
        owner.replace_state(JudgeEngine::from_snapshot(&state).unwrap(), transport());
        let remaining = owner
            .advance_to(point(1, 1021), &NoMapping, point(2, 3))
            .unwrap();
        assert_eq!(
            remaining.hazard_events,
            [hazard(4, 21, 1, 4, outcome, None)]
        );
        assert!(consumer.try_pop().is_err());
    }
    let judge = engine(
        &[],
        i64::MAX,
        Box::new(ClosestCandidate),
        Some(vec![marker(1, i64::MAX, 1, 1)]),
    );
    let (mut overflow, _) = runtime(judge, &[1], 1, vec![]);
    overflow.set_song_end(ts(1)).unwrap();
    let first = overflow
        .process_input(button(0, 1, ButtonState::Down), &NoMapping, point(2, 0))
        .unwrap();
    assert_eq!(
        first.hazard_events,
        [hazard(
            1,
            i64::MAX,
            1,
            1,
            HazardOutcome::Triggered,
            Some(meta(0, 1))
        )]
    );
    let hash = overflow.judge().stable_hash().unwrap();
    let end_error = overflow
        .process_input(button(1, 2, ButtonState::Up), &NoMapping, point(2, 1))
        .unwrap();
    assert!(
        end_error.song_end_reached
            && end_error.bound_inputs.is_empty()
            && end_error.hazard_events.is_empty()
    );
    assert_eq!(end_error.judge_error, Some(JudgeError::Overflow));
    let advance_error = overflow
        .advance_to(point(1, 1002), &NoMapping, point(2, 2))
        .unwrap();
    assert_eq!(advance_error.judge_error, Some(JudgeError::Overflow));
    assert!(advance_error.hazard_events.is_empty());
    assert_eq!(overflow.judge().hazard_events(), first.hazard_events);
    assert_eq!(overflow.judge().stable_hash().unwrap(), hash);
}

#[test]
fn ordinary_audio_queue_rejection_preserves_hazards_without_turning_them_into_scores_or_commands() {
    for disconnected in [false, true] {
        let judge = engine(
            &[(1, 100, 1), (2, 100, 2)],
            0,
            Box::new(ClosestCandidate),
            Some(vec![marker(1, 100, 1, u64::MAX), marker(2, 100, 2, 1295)]),
        );
        let sounds = vec![
            SoundBinding {
                object: ObjectId(1),
                stage: JudgeStage::Instant,
                sample: SampleId(11),
                voice: VoiceId(21),
                gain: 1.0,
            },
            SoundBinding {
                object: ObjectId(2),
                stage: JudgeStage::Instant,
                sample: SampleId(12),
                voice: VoiceId(22),
                gain: 0.5,
            },
        ];
        let (mut owner, consumer) = runtime(judge, &[1, 2], 1, sounds);
        let mut consumer = Some(consumer);
        if disconnected {
            drop(consumer.take());
        }
        let report = owner
            .process_input(button(100, 1, ButtonState::Down), &NoMapping, point(2, -99))
            .unwrap();
        let first = AudioCommand::Play {
            sample: SampleId(11),
            voice: VoiceId(21),
            at: ts(-99),
            gain: 1.0,
        };
        let second = AudioCommand::Play {
            sample: SampleId(12),
            voice: VoiceId(22),
            at: ts(-99),
            gain: 0.5,
        };
        assert_eq!(
            report.hazard_events,
            [
                hazard(
                    1,
                    100,
                    1,
                    u64::MAX,
                    HazardOutcome::Triggered,
                    Some(meta(100, 1))
                ),
                hazard(2, 100, 2, 1295, HazardOutcome::Avoided, Some(meta(100, 1)))
            ]
        );
        assert_eq!(report.judge_events.len(), 2);
        assert!(report.judge_error.is_none());
        assert_eq!(owner.telemetry().counters().judge_results, 2);
        if disconnected {
            assert!(report.audio_commands.is_empty());
            assert_eq!(
                report
                    .audio_failures
                    .iter()
                    .map(|f| (f.command, f.reason))
                    .collect::<Vec<_>>(),
                [
                    (first, QueuePushError::Disconnected),
                    (second, QueuePushError::Disconnected)
                ]
            );
            assert_eq!(owner.telemetry().counters().queue_disconnected, 2);
        } else {
            assert_eq!(report.audio_commands, [first]);
            assert_eq!(report.audio_failures.len(), 1);
            assert_eq!(
                (
                    report.audio_failures[0].command,
                    report.audio_failures[0].reason
                ),
                (second, QueuePushError::Full)
            );
            assert_eq!(consumer.as_mut().unwrap().try_pop().unwrap(), first);
            assert!(consumer.as_mut().unwrap().try_pop().is_err());
            assert_eq!(owner.telemetry().counters().queue_full, 1);
        }
        let duplicate = owner
            .process_input(button(101, 2, ButtonState::Down), &NoMapping, point(2, 0))
            .unwrap();
        assert!(
            duplicate.hazard_events.is_empty()
                && duplicate.judge_events.is_empty()
                && duplicate.audio_commands.is_empty()
                && duplicate.audio_failures.is_empty()
        );
    }
}

#[test]
fn actual_recording_reconstruction_and_restored_owners_reproduce_binding_ordered_hazard_delivery() {
    let make = || {
        engine(
            &[],
            0,
            Box::new(ClosestCandidate),
            Some(vec![
                marker(1, 10, 1, 1),
                marker(2, 10, 2, 2),
                marker(3, 15, 1, 3),
                marker(4, 15, 2, 4),
                marker(5, 20, 1, 5),
                marker(6, 20, 2, 6),
                marker(7, 30, 1, 7),
                marker(8, 30, 2, 8),
                marker(9, 40, 1, 9),
                marker(10, 40, 2, 10),
                marker(11, 50, 1, 11),
                marker(12, 50, 2, 12),
            ]),
        )
    };
    let (mut owner, mut consumer) = runtime(make(), &[1, 2], 4, vec![]);
    let mut recorder = ReplayRecorder::new(header()).unwrap();
    let mut replay = ReplaySession::new(header(), make()).unwrap();
    let operations = [
        Some(button(10, 1, ButtonState::Down)),
        Some(button(11, 2, ButtonState::Repeat)),
        Some(button(20, 3, ButtonState::Up)),
        None,
        Some(touch(40, 4, TouchPhase::Down)),
        Some(touch(50, 5, TouchPhase::Cancel)),
    ];
    for (index, operation) in operations.into_iter().enumerate() {
        let report = match operation {
            Some(input) => owner
                .process_input(input, &NoMapping, point(2, index as i64))
                .unwrap(),
            None => owner
                .advance_to(point(1, 1030), &NoMapping, point(2, index as i64))
                .unwrap(),
        };
        assert!(
            report.judge_error.is_none()
                && report.audio_commands.is_empty()
                && report.audio_failures.is_empty()
        );
        let expected = match index {
            0 => vec![
                hazard(1, 10, 1, 1, HazardOutcome::Triggered, Some(meta(10, 1))),
                hazard(2, 10, 2, 2, HazardOutcome::Avoided, Some(meta(10, 1))),
            ],
            1 => vec![],
            2 => vec![
                hazard(3, 15, 1, 3, HazardOutcome::Triggered, None),
                hazard(4, 15, 2, 4, HazardOutcome::Triggered, None),
                hazard(5, 20, 1, 5, HazardOutcome::Avoided, Some(meta(20, 3))),
                hazard(6, 20, 2, 6, HazardOutcome::Triggered, Some(meta(20, 3))),
            ],
            3 => vec![
                hazard(7, 30, 1, 7, HazardOutcome::Avoided, None),
                hazard(8, 30, 2, 8, HazardOutcome::Avoided, None),
            ],
            4 => vec![
                hazard(9, 40, 1, 9, HazardOutcome::Triggered, Some(meta(40, 4))),
                hazard(10, 40, 2, 10, HazardOutcome::Avoided, Some(meta(40, 4))),
            ],
            5 => vec![
                hazard(11, 50, 1, 11, HazardOutcome::Avoided, Some(meta(50, 5))),
                hazard(12, 50, 2, 12, HazardOutcome::Triggered, Some(meta(50, 5))),
            ],
            _ => unreachable!(),
        };
        assert_eq!(report.hazard_events, expected);
        recorder.record_report(&report).unwrap();
        let mut reconstructed = Vec::new();
        for event in &report.bound_inputs {
            assert!(
                replay
                    .push_input(event.clone(), report.song_time)
                    .unwrap()
                    .is_empty()
            );
            reconstructed.extend_from_slice(replay.engine().hazard_events());
        }
        if report.input.is_none() {
            assert!(replay.advance_to(report.song_time).unwrap().is_empty());
            reconstructed.extend_from_slice(replay.engine().hazard_events());
        }
        assert_eq!(reconstructed, expected);
        assert_eq!(
            owner.judge().stable_hash().unwrap(),
            replay.engine().stable_hash().unwrap()
        );
        if index == 0 {
            replay.checkpoint().unwrap();
        }
    }
    assert_eq!(recorder.records().len(), 11);
    let (header, records) = recorder.into_parts();
    let loaded = ReplaySession::from_records(header, make(), records).unwrap();
    assert_eq!(
        loaded.engine().stable_hash().unwrap(),
        owner.judge().stable_hash().unwrap()
    );
    replay.seek_cursor(2).unwrap(); // Both actual destinations from the first Down remain held.
    owner.replace_state(
        JudgeEngine::from_snapshot(&replay.engine().snapshot().unwrap()).unwrap(),
        transport(),
    );
    let duplicate = owner
        .process_input(button(10, 1, ButtonState::Down), &NoMapping, point(2, 10))
        .unwrap();
    assert!(duplicate.hazard_events.is_empty());
    let release = owner
        .process_input(button(20, 2, ButtonState::Up), &NoMapping, point(2, 11))
        .unwrap();
    assert_eq!(
        release.hazard_events,
        [
            hazard(3, 15, 1, 3, HazardOutcome::Triggered, None),
            hazard(4, 15, 2, 4, HazardOutcome::Triggered, None),
            hazard(5, 20, 1, 5, HazardOutcome::Avoided, Some(meta(20, 2))),
            hazard(6, 20, 2, 6, HazardOutcome::Triggered, Some(meta(20, 2)))
        ]
    );
    assert!(consumer.try_pop().is_err());
}
