//! Deferred optional hazard sound fixtures using actual judges and queue owners.
//! No BMS damage policy, asset loader, mixer or native device is simulated here.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    runtime::{
        hazard_sound::{HazardSoundBinding, HazardSoundError, HazardSoundTimeline},
        input_sound::{InputSoundMarker, InputSoundTimeline},
        Runtime, RuntimeError, RuntimeProcessingClock, SoundBinding,
    },
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
            x: -900.5,
            y: 900.25,
        },
        pressure: Some(0.75),
    })
}
fn unbound(song: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(song, sequence),
        control: physical(99),
        state: ButtonState::Down,
    })
}
fn marker(id: u64, at: i64, control: u32, value: u64) -> HazardMarker {
    HazardMarker {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(control),
        value,
    }
}
fn event(
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
fn sound(id: u64, sample: u64, voice: u64, gain: f32) -> HazardSoundBinding {
    HazardSoundBinding {
        hazard: HazardId(id),
        sample: SampleId(sample),
        voice: VoiceId(voice),
        gain,
    }
}
fn sounds(bindings: Vec<HazardSoundBinding>) -> HazardSoundTimeline {
    let count = bindings.len();
    HazardSoundTimeline::new(bindings, count).unwrap()
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn transport() -> Transport {
    Transport::new(ts(1000), ts(0), Rate::NORMAL)
}
fn engine(
    notes: &[(u64, i64, u32)],
    markers: Vec<HazardMarker>,
    resolver: Box<dyn CandidateResolver>,
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
        Duration::ZERO,
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
    let count = markers.len();
    judge
        .configure_hazards(HazardTimeline::new(markers, count).unwrap())
        .unwrap();
    judge
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
fn normal_sound(object: u64) -> SoundBinding {
    SoundBinding {
        object: ObjectId(object),
        stage: JudgeStage::Instant,
        sample: SampleId(99),
        voice: VoiceId(999),
        gain: 1.0,
    }
}
fn presses() -> InputSoundTimeline {
    InputSoundTimeline::new(
        (0..4)
            .map(|control| InputSoundMarker {
                control: GameControlId(control),
                at: ts(0),
                sample: SampleId(10 + u64::from(control)),
                voice: VoiceId(100 + u64::from(control)),
                gain: -0.5,
            })
            .collect(),
        4,
    )
    .unwrap()
}

#[test]
fn exact_owned_bindings_validate_and_configuration_is_once_only_before_real_commit() {
    let timeline = sounds(vec![
        sound(u64::MAX, u64::MAX, u64::MAX, -2.5),
        sound(7, 3, 0, -0.0),
        sound(0, 0, 0, f32::MAX),
    ]);
    let retained = timeline.clone();
    let pointer = timeline.bindings().as_ptr();
    assert_eq!(
        timeline.bindings(),
        [
            sound(0, 0, 0, f32::MAX),
            sound(7, 3, 0, -0.0),
            sound(u64::MAX, u64::MAX, u64::MAX, -2.5)
        ]
    );
    let triggered = event(
        u64::MAX,
        i64::MIN,
        u32::MAX,
        u64::MAX,
        HazardOutcome::Triggered,
        Some(meta(0, u64::MAX)),
    );
    assert_eq!(
        timeline.command_for(&triggered, ts(i64::MAX)),
        Some(play(u64::MAX, u64::MAX, i64::MAX, -2.5))
    );
    assert_eq!(
        timeline.command_for(
            &HazardEvent {
                value: 0,
                at: ts(604_800_000_000_000),
                input: None,
                ..triggered
            },
            ts(-99)
        ),
        Some(play(u64::MAX, u64::MAX, -99, -2.5)),
        "opaque values and marker time are not audio policy"
    );
    assert_eq!(
        timeline.command_for(
            &HazardEvent {
                outcome: HazardOutcome::Avoided,
                ..triggered
            },
            ts(0)
        ),
        None
    );
    assert_eq!(
        timeline.command_for(
            &HazardEvent {
                id: HazardId(u64::MAX - 1),
                ..triggered
            },
            ts(0)
        ),
        None
    );
    let Some(AudioCommand::Play { gain, .. }) = timeline.command_for(
        &HazardEvent {
            id: HazardId(7),
            ..triggered
        },
        ts(0),
    ) else {
        panic!("known triggered identity")
    };
    assert_eq!(gain.to_bits(), (-0.0f32).to_bits());
    assert_eq!(timeline, retained);
    assert_eq!(timeline.bindings().as_ptr(), pointer);
    assert!(
        HazardSoundTimeline::new(vec![], 0)
            .unwrap()
            .bindings()
            .is_empty()
    );
    assert!(HazardSoundTimeline::new(vec![], usize::MAX).is_ok());
    assert_eq!(
        HazardSoundTimeline::new(vec![sound(1, 1, 1, 1.0)], 0).unwrap_err(),
        HazardSoundError::Capacity
    );
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            HazardSoundTimeline::new(vec![sound(u64::MAX, 1, 1, gain)], 1).unwrap_err(),
            HazardSoundError::InvalidGain {
                hazard: HazardId(u64::MAX)
            }
        );
    }
    assert_eq!(
        HazardSoundTimeline::new(vec![sound(7, 1, 1, 1.0), sound(7, 2, 2, 0.0)], 2).unwrap_err(),
        HazardSoundError::DuplicateHazard {
            hazard: HazardId(7)
        }
    );
    let make = || engine(&[], vec![marker(7, 10, 1, 0)], Box::new(ClosestCandidate));
    let (mut owner, mut consumer) = runtime(make(), &[1], 4, vec![]);
    let hash = owner.judge().stable_hash().unwrap();
    let mut unmapped = button(10, 1, ButtonState::Down);
    unmapped.meta_mut().clock_domain = ClockDomainId(99);
    assert!(matches!(
        owner.process_input(unmapped, &NoMapping, point(2, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert!(matches!(
        owner.process_input(button(10, 1, ButtonState::Down), &NoMapping, point(99, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    owner.configure_input_sounds(presses()).unwrap();
    owner
        .configure_hazard_sounds(sounds(vec![sound(7, 77, 88, 0.5)]))
        .unwrap();
    assert_eq!(owner.judge().stable_hash().unwrap(), hash);
    assert_eq!(
        owner.configure_hazard_sounds(sounds(vec![sound(7, 9, 9, 1.0)])),
        Err(HazardSoundError::AlreadyConfigured)
    );
    let report = owner
        .process_input(button(10, 1, ButtonState::Down), &NoMapping, point(2, -30))
        .unwrap();
    assert_eq!(
        report.audio_commands,
        [play(11, 101, -30, -0.5), play(77, 88, -30, 0.5)]
    );
    for expected in &report.audio_commands {
        assert_eq!(consumer.try_pop().unwrap(), *expected);
    }
    assert_eq!(
        owner.configure_hazard_sounds(sounds(vec![])),
        Err(HazardSoundError::AlreadyConfigured)
    );
    for advance in [false, true] {
        let (mut owner, mut consumer) = runtime(make(), &[1], 4, vec![]);
        let pristine = owner.judge().snapshot().unwrap();
        let committed = if advance {
            owner
                .advance_to(point(1, 1001), &NoMapping, point(2, 0))
                .unwrap()
        } else {
            owner
                .process_input(unbound(1, 1), &NoMapping, point(2, 0))
                .unwrap()
        };
        assert!(committed.audio_commands.is_empty());
        assert_eq!(
            owner.configure_hazard_sounds(sounds(vec![])),
            Err(HazardSoundError::AlreadyStarted)
        );
        owner.replace_state(JudgeEngine::from_snapshot(&pristine).unwrap(), transport());
        assert_eq!(
            owner.configure_hazard_sounds(sounds(vec![])),
            Err(HazardSoundError::AlreadyStarted)
        );
        assert!(consumer.try_pop().is_err());
    }
    let (mut configured_empty, mut queue) = runtime(make(), &[1], 1, vec![]);
    configured_empty
        .configure_hazard_sounds(sounds(vec![]))
        .unwrap();
    assert_eq!(
        configured_empty.configure_hazard_sounds(sounds(vec![sound(7, 1, 1, 1.0)])),
        Err(HazardSoundError::AlreadyConfigured)
    );
    let silent = configured_empty
        .process_input(button(10, 1, ButtonState::Down), &NoMapping, point(2, 0))
        .unwrap();
    assert_eq!(silent.hazard_events.len(), 1);
    assert!(
        silent.audio_commands.is_empty()
            && silent.audio_failures.is_empty()
            && queue.try_pop().is_err()
    );
}

#[test]
fn real_fanout_publishes_hits_then_press_fallbacks_then_actual_hazard_order_at_output_time() {
    let make = || {
        engine(
            &[(2, 100, 2)],
            vec![
                marker(u64::MAX, 100, 1, u64::MAX),
                marker(0, 100, 2, 1295),
                marker(9, 100, 1, 0),
                marker(10, 100, 99, 1),
            ],
            Box::new(ClosestCandidate),
        )
    };
    let (mut owner, mut consumer) = runtime(make(), &[1, 2, 3], 8, vec![normal_sound(2)]);
    owner.configure_input_sounds(presses()).unwrap();
    owner
        .configure_hazard_sounds(sounds(vec![
            sound(9, 200, 500, 0.25),
            sound(0, 202, 502, 1.0),
            sound(u64::MAX, 201, 500, -1.0),
        ]))
        .unwrap();
    let mapper = AffineClockMapper::exact_offset(
        ClockPair {
            source: point(8, 0),
            target: point(2, 9_007_199_254_740_993),
        },
        ClockInterval {
            start: ts(0),
            end: ts(100),
        },
    )
    .unwrap();
    let report = owner
        .process_input(
            button(100, u64::MAX, ButtonState::Down),
            &mapper,
            point(8, 7),
        )
        .unwrap();
    assert_eq!(report.song_time, ts(100));
    assert_eq!(report.audio_at, point(2, 9_007_199_254_741_000));
    assert_eq!(
        report
            .bound_inputs
            .iter()
            .map(|input| input.game_control.0)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        report.hazard_events,
        [
            event(
                u64::MAX,
                100,
                1,
                u64::MAX,
                HazardOutcome::Triggered,
                Some(meta(100, u64::MAX))
            ),
            event(
                0,
                100,
                2,
                1295,
                HazardOutcome::Avoided,
                Some(meta(100, u64::MAX))
            ),
            event(
                9,
                100,
                1,
                0,
                HazardOutcome::Triggered,
                Some(meta(100, u64::MAX))
            ),
            event(
                10,
                100,
                99,
                1,
                HazardOutcome::Avoided,
                Some(meta(100, u64::MAX))
            ),
        ]
    );
    assert_eq!(
        report.audio_commands,
        [
            play(99, 999, 9_007_199_254_741_000, 1.0),
            play(11, 101, 9_007_199_254_741_000, -0.5),
            play(13, 103, 9_007_199_254_741_000, -0.5),
            play(201, 500, 9_007_199_254_741_000, -1.0),
            play(200, 500, 9_007_199_254_741_000, 0.25),
        ]
    );
    assert!(report.audio_failures.is_empty() && report.judge_error.is_none());
    assert_eq!(report.judge_events.len(), 1);
    assert!(
        owner.judge().hazard_events().is_empty(),
        "later successful fanout clears the judge scratch, not the Runtime report"
    );
    for expected in &report.audio_commands {
        assert_eq!(consumer.try_pop().unwrap(), *expected);
    }
    assert!(consumer.try_pop().is_err());
    assert_eq!(
        (
            owner.telemetry().counters().judge_results,
            owner.telemetry().counters().audio_commands
        ),
        (1, 5)
    );
    let (mut legacy, mut legacy_queue) = runtime(make(), &[1, 2, 3], 8, vec![normal_sound(2)]);
    legacy.configure_input_sounds(presses()).unwrap();
    let unchanged = legacy
        .process_input(
            button(100, u64::MAX, ButtonState::Down),
            &mapper,
            point(8, 7),
        )
        .unwrap();
    assert_eq!(unchanged.audio_commands, report.audio_commands[..3]);
    assert_eq!(unchanged.hazard_events, report.hazard_events);
    assert_eq!(
        legacy.judge().stable_hash().unwrap(),
        owner.judge().stable_hash().unwrap()
    );
    for expected in &unchanged.audio_commands {
        assert_eq!(legacy_queue.try_pop().unwrap(), *expected);
    }
}

#[test]
fn held_buttons_and_enabled_contacts_sound_only_consumed_triggered_boundaries_including_finite_end()
{
    for contact in [false, true] {
        let judge = engine(
            &[],
            (1..=6)
                .map(|id| marker(id, id as i64 * 10, 1, id))
                .collect(),
            Box::new(ClosestCandidate),
        );
        let (mut owner, mut queue) = runtime(judge, &[1], 8, vec![]);
        owner
            .configure_hazard_sounds(sounds(
                (1..=6).map(|id| sound(id, 100 + id, 9, 0.5)).collect(),
            ))
            .unwrap();
        owner.set_song_end(ts(50)).unwrap();
        let input = |song, sequence, phase| {
            if contact {
                touch(
                    song,
                    sequence,
                    match phase {
                        0 => TouchPhase::Down,
                        1 => TouchPhase::Move,
                        _ => TouchPhase::Cancel,
                    },
                )
            } else {
                button(
                    song,
                    sequence,
                    match phase {
                        0 => ButtonState::Down,
                        1 => ButtonState::Repeat,
                        _ => ButtonState::Up,
                    },
                )
            }
        };
        let down = owner
            .process_input(input(10, 1, 0), &NoMapping, point(2, -10))
            .unwrap();
        assert_eq!(down.audio_commands, [play(101, 9, -10, 0.5)]);
        assert_eq!(
            down.hazard_events,
            [event(
                1,
                10,
                1,
                1,
                HazardOutcome::Triggered,
                Some(meta(10, 1))
            )]
        );
        let held = owner
            .process_input(input(15, 2, 1), &NoMapping, point(2, -9))
            .unwrap();
        assert!(held.hazard_events.is_empty() && held.audio_commands.is_empty());
        let advanced = owner
            .advance_to(point(1, 1020), &NoMapping, point(2, -8))
            .unwrap();
        assert_eq!(
            advanced.hazard_events,
            [event(2, 20, 1, 2, HazardOutcome::Triggered, None)]
        );
        assert_eq!(advanced.audio_commands, [play(102, 9, -8, 0.5)]);
        let released = owner
            .process_input(input(30, 3, 2), &NoMapping, point(2, -7))
            .unwrap();
        assert_eq!(
            released.hazard_events,
            [event(
                3,
                30,
                1,
                3,
                HazardOutcome::Avoided,
                Some(meta(30, 3))
            )]
        );
        assert!(released.audio_commands.is_empty());
        let next = owner
            .process_input(input(40, 4, 0), &NoMapping, point(2, -6))
            .unwrap();
        assert_eq!(next.audio_commands, [play(104, 9, -6, 0.5)]);
        let ended = owner
            .process_input(input(50, 5, 2), &NoMapping, point(2, -5))
            .unwrap();
        assert!(ended.song_end_reached && ended.input.is_none() && ended.bound_inputs.is_empty());
        assert_eq!(
            ended.hazard_events,
            [event(5, 50, 1, 5, HazardOutcome::Triggered, None)]
        );
        assert_eq!(ended.audio_commands, [play(105, 9, -5, 0.5)]);
        let repeated = owner
            .advance_to(point(1, 1100), &NoMapping, point(2, -4))
            .unwrap();
        assert!(
            repeated.song_end_reached
                && repeated.hazard_events.is_empty()
                && repeated.audio_commands.is_empty()
        );
        assert_eq!(owner.judge().remaining_hazards(), 1);
        for expected in [
            play(101, 9, -10, 0.5),
            play(102, 9, -8, 0.5),
            play(104, 9, -6, 0.5),
            play(105, 9, -5, 0.5),
        ] {
            assert_eq!(queue.try_pop().unwrap(), expected);
        }
        assert!(queue.try_pop().is_err());
        assert_eq!(owner.telemetry().counters().judge_results, 0);
    }
}

#[test]
fn queue_failures_keep_every_committed_hazard_and_each_attempt_without_replaying_stale_reports() {
    let judge = engine(
        &[(1, 10, 1)],
        vec![
            marker(3, 10, 1, 3),
            marker(1, 10, 1, 1),
            marker(2, 10, 1, 2),
            marker(4, 20, 1, 4),
        ],
        Box::new(ClosestCandidate),
    );
    let (mut owner, mut queue) = runtime(judge, &[1], 2, vec![normal_sound(1)]);
    owner
        .configure_hazard_sounds(sounds(
            (1..=4).map(|id| sound(id, 100 + id, 9, 1.0)).collect(),
        ))
        .unwrap();
    let report = owner
        .process_input(button(10, 10, ButtonState::Down), &NoMapping, point(2, 700))
        .unwrap();
    assert_eq!(
        report
            .hazard_events
            .iter()
            .map(|e| e.id.0)
            .collect::<Vec<_>>(),
        [3, 1, 2]
    );
    assert!(
        report
            .hazard_events
            .iter()
            .all(|e| e.outcome == HazardOutcome::Triggered)
    );
    assert_eq!(
        report.audio_commands,
        [play(99, 999, 700, 1.0), play(103, 9, 700, 1.0)]
    );
    assert_eq!(
        report
            .audio_failures
            .iter()
            .map(|f| (f.command, f.reason))
            .collect::<Vec<_>>(),
        [
            (play(101, 9, 700, 1.0), QueuePushError::Full),
            (play(102, 9, 700, 1.0), QueuePushError::Full),
        ]
    );
    assert_eq!(
        (
            owner.telemetry().counters().audio_commands,
            owner.telemetry().counters().queue_full
        ),
        (2, 2)
    );
    assert_eq!(queue.try_pop().unwrap(), report.audio_commands[0]);
    assert_eq!(queue.try_pop().unwrap(), report.audio_commands[1]);
    let hash = owner.judge().stable_hash().unwrap();
    assert!(matches!(
        owner.process_input(button(11, 9, ButtonState::Up), &NoMapping, point(2, 701)),
        Err(RuntimeError::SequenceRegression { .. })
    ));
    assert!(matches!(
        owner.advance_to(point(1, 1011), &NoMapping, point(99, 701)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert_eq!(owner.judge().stable_hash().unwrap(), hash);
    assert_eq!(owner.judge().hazard_events(), report.hazard_events);
    let ignored = owner
        .process_input(unbound(11, 11), &NoMapping, point(2, 701))
        .unwrap();
    assert!(
        ignored.hazard_events.is_empty()
            && ignored.audio_commands.is_empty()
            && ignored.audio_failures.is_empty()
    );
    assert_eq!(owner.judge().hazard_events(), report.hazard_events);
    let empty = owner
        .advance_to(point(1, 1012), &NoMapping, point(2, 702))
        .unwrap();
    assert!(empty.hazard_events.is_empty() && empty.audio_commands.is_empty());
    assert!(
        queue.try_pop().is_err(),
        "neither failed nor empty operations retry queue failures"
    );
    drop(queue);
    let disconnected = owner
        .advance_to(point(1, 1020), &NoMapping, point(2, 703))
        .unwrap();
    assert_eq!(
        disconnected.hazard_events,
        [event(4, 20, 1, 4, HazardOutcome::Triggered, None)]
    );
    assert!(disconnected.audio_commands.is_empty());
    assert_eq!(disconnected.audio_failures.len(), 1);
    assert_eq!(
        disconnected.audio_failures[0].command,
        play(104, 9, 703, 1.0)
    );
    assert_eq!(
        disconnected.audio_failures[0].reason,
        QueuePushError::Disconnected
    );
    let later = owner
        .advance_to(point(1, 1021), &NoMapping, point(2, 704))
        .unwrap();
    assert!(
        later.hazard_events.is_empty()
            && later.audio_commands.is_empty()
            && later.audio_failures.is_empty()
    );
    assert_eq!(owner.telemetry().counters().queue_disconnected, 1);
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
        Some(b"hazard-sounds-reject-second/v1".to_vec())
    }
}

#[test]
fn later_judge_failure_publishes_only_actual_successful_prefix_and_first_failure_cannot_reuse_old_outcomes()
 {
    let make = || {
        engine(
            &[(1, 100, 1), (2, 100, 2)],
            vec![
                marker(0, 0, 0, 0),
                marker(3, 100, 0, 1295),
                marker(1, 100, 1, u64::MAX),
                marker(2, 100, 2, 1),
            ],
            Box::new(RejectSecond),
        )
    };
    let (mut owner, mut queue) = runtime(make(), &[0, 1, 2, 3], 8, vec![normal_sound(1)]);
    owner.configure_input_sounds(presses()).unwrap();
    owner
        .configure_hazard_sounds(sounds(vec![
            sound(3, 203, 777, -1.0),
            sound(1, 201, 777, -1.0),
            sound(2, 202, 777, -1.0),
        ]))
        .unwrap();
    let report = owner
        .process_input(button(100, 1, ButtonState::Down), &NoMapping, point(2, 800))
        .unwrap();
    assert_eq!(
        report.judge_error,
        Some(JudgeError::InvalidCandidate {
            object: ObjectId(999)
        })
    );
    assert_eq!(
        report
            .bound_inputs
            .iter()
            .map(|input| input.game_control.0)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(report.judge_events.len(), 1);
    assert_eq!(
        report.hazard_events,
        [
            event(0, 0, 0, 0, HazardOutcome::Avoided, None),
            event(
                3,
                100,
                0,
                1295,
                HazardOutcome::Triggered,
                Some(meta(100, 1))
            ),
            event(
                1,
                100,
                1,
                u64::MAX,
                HazardOutcome::Avoided,
                Some(meta(100, 1))
            ),
            event(2, 100, 2, 1, HazardOutcome::Avoided, Some(meta(100, 1))),
        ]
    );
    assert_eq!(
        report.audio_commands,
        [
            play(99, 999, 800, 1.0),
            play(10, 100, 800, -0.5),
            play(203, 777, 800, -1.0)
        ]
    );
    assert!(report.audio_failures.is_empty());
    for expected in &report.audio_commands {
        assert_eq!(queue.try_pop().unwrap(), *expected);
    }
    assert!(queue.try_pop().is_err());
    assert_eq!(owner.telemetry().counters().judge_results, 1);
    let (mut first_fails, mut queue) = runtime(make(), &[2, 0, 1], 4, vec![]);
    first_fails
        .configure_hazard_sounds(sounds(vec![sound(0, 200, 7, 1.0), sound(3, 203, 7, 1.0)]))
        .unwrap();
    let prior = first_fails
        .advance_to(point(1, 1000), &NoMapping, point(2, 0))
        .unwrap();
    assert_eq!(
        prior.hazard_events,
        [event(0, 0, 0, 0, HazardOutcome::Avoided, None)]
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
            && failed.hazard_events.is_empty()
            && failed.audio_commands.is_empty()
            && failed.audio_failures.is_empty()
    );
    assert_eq!(first_fails.judge().hazard_events(), prior.hazard_events);
    assert_eq!(first_fails.judge().stable_hash().unwrap(), hash);
    assert!(queue.try_pop().is_err());
}
