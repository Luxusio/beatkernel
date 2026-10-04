//! Deferred actual Runtime queues and ReplaySession reconstruction; no audio device.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::{PressHoldEvaluator, PressInstantEvaluator},
    judge::*,
    replay::{REPLAY_VERSION, ReplayHeader, ReplaySession},
    runtime::{
        input_sound::{InputSoundMarker, InputSoundTimeline},
        Runtime, RuntimeError, RuntimeProcessingClock, SoundBinding,
    },
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
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
fn physical() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(17),
        code: 5,
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
        control: physical(),
        state,
    })
}
fn touch(song: i64, sequence: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(song, sequence),
        control: physical(),
        contact: ContactId(u64::MAX),
        phase,
        position: Position2 { x: 2.5, y: -3.0 },
        pressure: None,
    })
}
fn bound(input: PhysicalInputEvent, control: u32) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: input,
    }
}
fn transport() -> Transport {
    Transport::new(ts(1000), ts(0), Rate::NORMAL)
}
fn engine(
    objects: &[(u64, i64, Option<i64>, u32)],
    offset: i64,
    resolver: Box<dyn CandidateResolver>,
) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = objects
        .iter()
        .map(|&(id, at, end, _)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(at).unwrap(),
            end: end.map(|end| Beat::new(end).unwrap()),
            interaction: InteractionId(id as u32),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    let rules = objects
        .iter()
        .map(|&(id, _, end, control)| Rule {
            interaction: InteractionId(id as u32),
            control: GameControlId(control),
            evaluator: if end.is_some() {
                Box::new(PressHoldEvaluator)
            } else {
                Box::new(PressInstantEvaluator)
            },
        })
        .collect();
    JudgeEngine::with_policies(
        source.compile().unwrap(),
        rules,
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::from_nanos(offset),
        )
        .unwrap(),
        resolver,
        Box::new(WindowJudgePolicy),
    )
    .unwrap()
}
fn runtime(
    judge: JudgeEngine,
    controls: &[u32],
    capacity: usize,
    sounds: Vec<SoundBinding>,
) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings(controls.iter().map(|&control| Binding {
        device: DeviceSelector::Any,
        physical: physical(),
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
fn marker(control: u32, at: i64, sample: u64, voice: u64, gain: f32) -> InputSoundMarker {
    InputSoundMarker {
        control: GameControlId(control),
        at: ts(at),
        sample: SampleId(sample),
        voice: VoiceId(voice),
        gain,
    }
}
fn timeline() -> InputSoundTimeline {
    InputSoundTimeline::new(
        vec![
            marker(1, 0, 11, 101, -0.5),
            marker(2, 0, 12, 102, 0.25),
            marker(3, 0, 13, 103, 1.0),
        ],
        3,
    )
    .unwrap()
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn basic() -> JudgeEngine {
    engine(&[(2, 100, None, 2)], 0, Box::new(ClosestCandidate))
}

#[test]
fn input_sounds_are_explicit_once_only_before_commit_and_end_or_advances_do_not_synthesize_presses()
{
    for operation in ["bound", "unbound", "advance"] {
        let (mut owner, mut consumer) = runtime(basic(), &[1], 8, vec![]);
        let pristine = owner.judge().snapshot().unwrap();
        let report = if operation == "advance" {
            owner
                .advance_to(point(1, 1010), &NoMapping, point(2, 300))
                .unwrap()
        } else {
            let mut input = button(10, 1, ButtonState::Down);
            if operation == "unbound" {
                if let PhysicalInputEvent::Button(button) = &mut input {
                    button.control = PhysicalControlId::keyboard(99);
                }
            }
            owner
                .process_input(input, &NoMapping, point(2, 300))
                .unwrap()
        };
        assert!(report.audio_commands.is_empty() && report.audio_failures.is_empty());
        assert!(consumer.try_pop().is_err());
        assert_eq!(
            owner.configure_input_sounds(timeline()),
            Err(RuntimeError::InputSoundConfigurationLocked)
        );
        owner.replace_state(JudgeEngine::from_snapshot(&pristine).unwrap(), transport());
        assert_eq!(
            owner.configure_input_sounds(timeline()),
            Err(RuntimeError::InputSoundConfigurationLocked),
            "restore cannot reopen late first configuration"
        );
        let (producer, _fresh_consumer) = command_queue(8).unwrap();
        owner.replace_session(
            JudgeEngine::from_snapshot(&pristine).unwrap(),
            transport(),
            producer,
        );
        assert_eq!(
            owner.configure_input_sounds(timeline()),
            Err(RuntimeError::InputSoundConfigurationLocked)
        );
    }
    let (mut owner, mut consumer) = runtime(basic(), &[1], 8, vec![]);
    let before = owner.judge().stable_hash().unwrap();
    let mut unmapped = button(10, 1, ButtonState::Down);
    unmapped.meta_mut().clock_domain = ClockDomainId(99);
    assert!(matches!(
        owner.process_input(unmapped, &NoMapping, point(2, 1)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert_eq!(owner.judge().stable_hash().unwrap(), before);
    owner.configure_input_sounds(timeline()).unwrap();
    assert_eq!(
        owner.configure_input_sounds(InputSoundTimeline::new(vec![], 1).unwrap()),
        Err(RuntimeError::InputSoundConfigurationLocked)
    );
    owner.set_song_end(ts(100)).unwrap();
    let first = owner
        .process_input(button(10, 1, ButtonState::Down), &NoMapping, point(2, -99))
        .unwrap();
    assert_eq!(first.audio_commands, [play(11, 101, -99, -0.5)]);
    assert_eq!(consumer.try_pop().unwrap(), play(11, 101, -99, -0.5));
    owner
        .process_input(button(11, 2, ButtonState::Up), &NoMapping, point(2, 1))
        .unwrap();
    let ended = owner
        .process_input(button(100, 3, ButtonState::Down), &NoMapping, point(2, 2))
        .unwrap();
    assert!(ended.song_end_reached && ended.input.is_none() && ended.bound_inputs.is_empty());
    assert!(ended.audio_commands.is_empty() && ended.audio_failures.is_empty());
    let advanced = owner
        .advance_to(point(1, 1200), &NoMapping, point(2, 3))
        .unwrap();
    assert!(advanced.audio_commands.is_empty() && advanced.audio_failures.is_empty());
    assert!(consumer.try_pop().is_err());
}

struct InvalidCandidate;
impl CandidateResolver for InvalidCandidate {
    fn select(&self, _: &[Candidate]) -> Option<ObjectId> {
        Some(ObjectId(999))
    }
}

#[test]
fn actual_hit_commands_precede_bound_fallbacks_and_queue_or_later_fanout_failure_preserves_only_accepted_evidence()
 {
    let sounds = vec![SoundBinding {
        object: ObjectId(2),
        stage: JudgeStage::Instant,
        sample: SampleId(99),
        voice: VoiceId(999),
        gain: 1.0,
    }];
    let (mut owner, mut consumer) = runtime(basic(), &[1, 2, 3], 2, sounds);
    owner.configure_input_sounds(timeline()).unwrap();
    let input = button(100, 1, ButtonState::Down);
    let report = owner
        .process_input(input.clone(), &NoMapping, point(2, 700))
        .unwrap();
    assert_eq!(
        report
            .bound_inputs
            .iter()
            .map(|event| event.game_control.0)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(report.judge_events.len(), 1);
    assert!(report.judge_error.is_none());
    assert_eq!(
        report.audio_commands,
        [play(99, 999, 700, 1.0), play(11, 101, 700, -0.5)]
    );
    assert_eq!(report.audio_failures.len(), 1);
    assert_eq!(report.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(report.audio_failures[0].command, play(13, 103, 700, 1.0));
    assert_eq!(consumer.try_pop().unwrap(), report.audio_commands[0]);
    assert_eq!(consumer.try_pop().unwrap(), report.audio_commands[1]);
    assert!(consumer.try_pop().is_err());
    for control in [1, 2, 3] {
        assert!(!owner.judge().is_fresh_press(&bound(input.clone(), control)));
    }
    let duplicate = owner
        .process_input(button(101, 2, ButtonState::Down), &NoMapping, point(2, 701))
        .unwrap();
    assert!(duplicate.audio_commands.is_empty() && duplicate.audio_failures.is_empty());
    assert!(consumer.try_pop().is_err());
    owner
        .process_input(button(102, 3, ButtonState::Up), &NoMapping, point(2, 702))
        .unwrap();
    drop(consumer);
    let disconnected = owner
        .process_input(button(103, 4, ButtonState::Down), &NoMapping, point(2, 703))
        .unwrap();
    assert!(disconnected.audio_commands.is_empty());
    assert_eq!(disconnected.audio_failures.len(), 3);
    assert!(
        disconnected
            .audio_failures
            .iter()
            .all(|failure| failure.reason == QueuePushError::Disconnected)
    );
    assert_eq!(owner.telemetry().counters().queue_full, 1);
    assert_eq!(owner.telemetry().counters().queue_disconnected, 3);
    assert_eq!(owner.telemetry().counters().audio_commands, 2);

    // The first destination has no note; the second reaches the real judge's
    // InvalidCandidate refusal. No custom judge or fabricated report is used.
    let failing = engine(&[(2, 100, None, 2)], 0, Box::new(InvalidCandidate));
    let (mut owner, mut consumer) = runtime(failing, &[1, 2, 3], 8, vec![]);
    owner.configure_input_sounds(timeline()).unwrap();
    let report = owner
        .process_input(input.clone(), &NoMapping, point(2, 800))
        .unwrap();
    assert_eq!(
        report.judge_error,
        Some(JudgeError::InvalidCandidate {
            object: ObjectId(999)
        })
    );
    assert_eq!(report.bound_inputs, [bound(input.clone(), 1)]);
    assert!(report.judge_events.is_empty());
    assert_eq!(report.audio_commands, [play(11, 101, 800, -0.5)]);
    assert!(report.audio_failures.is_empty());
    assert_eq!(consumer.try_pop().unwrap(), play(11, 101, 800, -0.5));
    assert!(consumer.try_pop().is_err());
    assert!(!owner.judge().is_fresh_press(&bound(input.clone(), 1)));
    assert!(owner.judge().is_fresh_press(&bound(input.clone(), 2)));
    assert!(owner.judge().is_fresh_press(&bound(input, 3)));
}

#[test]
fn real_replay_reconstruction_and_same_chart_restoration_retain_stateless_lookup_and_actual_press_ownership()
 {
    let make_judge = || {
        engine(
            &[(1, 107, None, 1), (2, 207, Some(307), 1)],
            7,
            Box::new(ClosestCandidate),
        )
    };
    let sounds = vec![
        SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(91),
            voice: VoiceId(201),
            gain: 1.0,
        },
        SoundBinding {
            object: ObjectId(2),
            stage: JudgeStage::HoldHead,
            sample: SampleId(92),
            voice: VoiceId(202),
            gain: 0.5,
        },
    ];
    let timeline = InputSoundTimeline::new(
        vec![
            marker(1, 0, 11, 101, -0.5),
            marker(1, 15, 66, 101, -0.5),
            marker(1, 150, 12, 101, -0.5),
            marker(1, 300, 13, 101, -0.5),
        ],
        4,
    )
    .unwrap();
    let (mut owner, mut consumer) = runtime(make_judge(), &[1], 8, sounds.clone());
    owner.configure_input_sounds(timeline.clone()).unwrap();
    let mut replay = ReplaySession::new(
        ReplayHeader {
            version: REPLAY_VERSION,
            chart_identity: b"input-sound-fixture".to_vec(),
            rules_identity: b"actual-press-rules".to_vec(),
            options: vec![],
            seed: 0,
            normalized_clock: ClockDomainId(1),
        },
        make_judge(),
    )
    .unwrap();
    let operations = [
        (10, button(10, 1, ButtonState::Down), Some((11, 101, -0.5))),
        (11, button(11, 2, ButtonState::Down), None),
        (12, button(12, 3, ButtonState::Repeat), None),
        (13, button(13, 4, ButtonState::Up), None),
        (100, button(100, 5, ButtonState::Down), Some((91, 201, 1.0))),
        (101, button(101, 6, ButtonState::Up), None),
        (200, touch(200, 7, TouchPhase::Down), Some((92, 202, 0.5))),
        (201, touch(201, 8, TouchPhase::Down), None),
        (202, touch(202, 9, TouchPhase::Move), None),
        (203, touch(203, 10, TouchPhase::Cancel), None),
        (204, touch(204, 11, TouchPhase::Down), Some((12, 101, -0.5))),
        (205, touch(205, 12, TouchPhase::Up), None),
        (
            300,
            button(300, 13, ButtonState::Down),
            Some((13, 101, -0.5)),
        ),
        (301, button(301, 14, ButtonState::Up), None),
    ];
    for (index, (song, input, expected)) in operations.into_iter().enumerate() {
        let output = 9_007_199_254_740_993 + index as i64;
        let report = owner
            .process_input(input.clone(), &NoMapping, point(2, output))
            .unwrap();
        assert!(report.judge_error.is_none() && report.audio_failures.is_empty());
        assert_eq!(report.song_time, ts(song));
        assert_eq!(report.audio_at, point(2, output));
        assert_eq!(report.bound_inputs, [bound(input, 1)]);
        let event = &report.bound_inputs[0];
        let fresh = replay.engine().is_fresh_press(event);
        let results = replay.push_input(event.clone(), ts(song)).unwrap();
        assert_eq!(results, report.judge_events);
        let mut replay_commands: Vec<_> = results
            .iter()
            .flat_map(|result| {
                sounds
                    .iter()
                    .filter_map(move |sound| sound.command_for(result, ts(output)))
            })
            .collect();
        replay_commands.extend(timeline.command_for_press(
            event,
            fresh,
            ts(song),
            ts(output),
            &results,
        ));
        let literal: Vec<_> = expected
            .map(|(sample, voice, gain)| play(sample, voice, output, gain))
            .into_iter()
            .collect();
        assert_eq!(report.audio_commands, literal);
        assert_eq!(replay_commands, literal);
        for command in literal {
            assert_eq!(consumer.try_pop().unwrap(), command);
        }
        assert!(consumer.try_pop().is_err());
        assert_eq!(
            owner.judge().stable_hash().unwrap(),
            replay.engine().stable_hash().unwrap()
        );
        if index == 0 || index == 10 {
            replay.checkpoint().unwrap();
        }
    }
    replay.seek_cursor(1).unwrap(); // Genuine first Down remains held in this actual checkpoint.
    owner.replace_state(
        JudgeEngine::from_snapshot(&replay.engine().snapshot().unwrap()).unwrap(),
        transport(),
    );
    assert_eq!(
        owner.configure_input_sounds(timeline.clone()),
        Err(RuntimeError::InputSoundConfigurationLocked)
    );
    let duplicate = owner
        .process_input(
            button(10, 1, ButtonState::Down),
            &NoMapping,
            point(2, 9_007_199_254_741_100),
        )
        .unwrap();
    assert!(duplicate.audio_commands.is_empty());
    owner
        .process_input(
            button(11, 2, ButtonState::Up),
            &NoMapping,
            point(2, 9_007_199_254_741_101),
        )
        .unwrap();
    let rearmed = owner
        .process_input(
            button(12, 3, ButtonState::Down),
            &NoMapping,
            point(2, 9_007_199_254_741_102),
        )
        .unwrap();
    assert_eq!(
        rearmed.audio_commands,
        [play(11, 101, 9_007_199_254_741_102, -0.5)]
    );
    assert_eq!(consumer.try_pop().unwrap(), rearmed.audio_commands[0]);
    assert!(consumer.try_pop().is_err());
    replay.seek_cursor(11).unwrap(); // The actual full-u64 contact remains held after its second Down.
    owner.replace_state_with_touch_router(
        JudgeEngine::from_snapshot(&replay.engine().snapshot().unwrap()).unwrap(),
        transport(),
        None,
    );
    let duplicate = owner
        .process_input(
            touch(204, 1, TouchPhase::Down),
            &NoMapping,
            point(2, 9_007_199_254_741_103),
        )
        .unwrap();
    assert!(duplicate.audio_commands.is_empty());
    assert!(consumer.try_pop().is_err());
    replay.seek_cursor(0).unwrap();
    let (producer, mut fresh_consumer) = command_queue(8).unwrap();
    owner.replace_session(
        JudgeEngine::from_snapshot(&replay.engine().snapshot().unwrap()).unwrap(),
        transport(),
        producer,
    );
    let fresh = owner
        .process_input(button(10, 1, ButtonState::Down), &NoMapping, point(2, -5))
        .unwrap();
    assert_eq!(fresh.audio_commands, [play(11, 101, -5, -0.5)]);
    assert_eq!(fresh_consumer.try_pop().unwrap(), fresh.audio_commands[0]);
    assert!(fresh_consumer.try_pop().is_err());
}
