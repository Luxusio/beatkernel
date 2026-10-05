//! Deferred software-owner fixtures; no gauge policy or output-drain claim.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::{InputOwner, InteractionState, PressHoldEvaluator, PressInstantEvaluator},
    judge::*,
    runtime::{
        Runtime, RuntimeError, RuntimeProcessingClock, RuntimeReport, SoundBinding,
        input_sound::{InputSoundMarker, InputSoundTimeline},
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
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
struct Clocks;
impl ClockMapper for Clocks {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        match (from.domain.0, to.0) {
            (9, 1) => from.timestamp.checked_add(Duration::from_nanos(1000)),
            (8, 2) => from.timestamp.checked_add(Duration::from_nanos(500)),
            _ => None,
        }
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn physical() -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(17),
        code: u32::MAX,
    }
}
fn meta(at: i64, sequence: u64) -> EventMeta {
    let mut result = EventMeta::new(DeviceId(u64::MAX), point(1, 1000 + at), sequence);
    result.native = Some(NativeEventMeta {
        backend: BackendId(17),
        code: Some(u32::MAX),
        timestamp: Some(point(7, -99)),
    });
    result
}
fn button(at: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(at, sequence),
        control: physical(),
        state,
    })
}
fn touch(at: i64, sequence: u64, contact: u64, phase: TouchPhase) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(at, sequence),
        control: physical(),
        contact: ContactId(contact),
        phase,
        position: Position2 {
            x: -700.5,
            y: 900.25,
        },
        pressure: Some(0.75),
    })
}
fn transport() -> Transport {
    Transport::new(ts(1000), ts(0), Rate::NORMAL)
}
fn judge(resolver: Box<dyn CandidateResolver>) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = [(1, 10, Some(100), 1), (2, 200, None, 2)]
        .map(|(id, at, end, rule)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(at).unwrap(),
            end: end.map(|n| Beat::new(n).unwrap()),
            interaction: InteractionId(rule),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .to_vec();
    let mut judge = JudgeEngine::with_policies_and_contacts(
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
        resolver,
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    judge
        .configure_hazards(
            HazardTimeline::new(
                vec![
                    HazardMarker {
                        id: HazardId(7),
                        at: ts(10),
                        control: GameControlId(1),
                        value: 50,
                    },
                    HazardMarker {
                        id: HazardId(u64::MAX),
                        at: ts(50),
                        control: GameControlId(1),
                        value: 1295,
                    },
                ],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    judge
}
fn fixture(capacity: usize) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings([1, 2].map(|control| Binding {
        device: DeviceSelector::Exact(DeviceId(u64::MAX)),
        physical: physical(),
        game_control: GameControlId(control),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut owner = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(),
        bindings,
        judge(Box::new(ClosestCandidate)),
        producer,
        vec![
            SoundBinding {
                object: ObjectId(1),
                stage: JudgeStage::HoldHead,
                sample: SampleId(1),
                voice: VoiceId(1),
                gain: 1.0,
            },
            SoundBinding {
                object: ObjectId(1),
                stage: JudgeStage::HoldTail,
                sample: SampleId(2),
                voice: VoiceId(2),
                gain: 1.0,
            },
        ],
        8,
    )
    .unwrap();
    owner.set_processing_clock(RuntimeProcessingClock::Disabled);
    owner
        .configure_input_sounds(
            InputSoundTimeline::new(
                vec![InputSoundMarker {
                    control: GameControlId(2),
                    at: ts(0),
                    sample: SampleId(3),
                    voice: VoiceId(3),
                    gain: -0.5,
                }],
                1,
            )
            .unwrap(),
        )
        .unwrap();
    owner
        .configure_hazard_sounds(
            HazardSoundTimeline::new(
                vec![
                    HazardSoundBinding {
                        hazard: HazardId(7),
                        sample: SampleId(4),
                        voice: VoiceId(4),
                        gain: 0.5,
                    },
                    HazardSoundBinding {
                        hazard: HazardId(u64::MAX),
                        sample: SampleId(5),
                        voice: VoiceId(5),
                        gain: 0.25,
                    },
                ],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    (owner, consumer)
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn frozen(report: &RuntimeReport, at: i64) {
    assert_eq!(report.song_time, ts(at));
    assert!(
        report.bound_inputs.is_empty()
            && report.judge_events.is_empty()
            && report.hazard_events.is_empty()
    );
    assert!(
        report.judge_error.is_none()
            && report.audio_commands.is_empty()
            && report.audio_failures.is_empty()
    );
}

#[test]
fn committed_button_hold_hazards_and_admitted_audio_survive_fencing_without_new_gameplay() {
    let (mut owner, mut consumer) = fixture(8);
    assert_eq!(owner.fence_gameplay(), None);
    assert_eq!(owner.gameplay_fence(), None);
    owner.set_song_end(ts(250)).unwrap(); // A pre-operation fence did not lock setup.
    let anchors = owner.transport().anchors().to_vec();
    let accepted = owner
        .process_input(button(10, 1, ButtonState::Down), &Clocks, point(2, 700))
        .unwrap();
    assert_eq!(accepted.judge_events.len(), 1);
    assert_eq!(accepted.judge_events[0].stage, JudgeStage::HoldHead);
    assert_eq!(accepted.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(
        accepted.audio_commands,
        [
            play(1, 1, 700, 1.0),
            play(3, 3, 700, -0.5),
            play(4, 4, 700, 0.5)
        ]
    );
    let hash = owner.judge().stable_hash().unwrap();
    let last_hazards = owner.judge().hazard_events().to_vec();
    assert_eq!(owner.fence_gameplay(), Some(ts(10)));
    for (at, sequence, state) in [
        (20, 2, ButtonState::Up),
        (30, 3, ButtonState::Down),
        (100, 4, ButtonState::Repeat),
    ] {
        let input = button(at, sequence, state);
        let report = owner
            .process_input(input.clone(), &Clocks, point(2, 900 + at))
            .unwrap();
        frozen(&report, 10);
        assert_eq!(report.input, Some(input));
        assert!(!report.song_end_reached);
        assert_eq!(owner.fence_gameplay(), Some(ts(10)));
    }
    let later = owner
        .advance_to(point(1, 2000), &Clocks, point(2, 4000))
        .unwrap();
    frozen(&later, 10);
    assert!(later.input.is_none() && later.song_end_reached);
    assert_eq!(
        owner
            .process_input(button(999, 99, ButtonState::Down), &Clocks, point(2, 4001))
            .unwrap_err(),
        RuntimeError::NonMonotonicHost
    );
    assert_eq!(owner.judge().stable_hash().unwrap(), hash);
    assert_eq!(owner.judge().hazard_events(), last_hazards);
    assert_eq!(
        owner.judge().state(ObjectId(1)),
        Some(InteractionState::Active)
    );
    assert_eq!(
        owner.judge().state(ObjectId(2)),
        Some(InteractionState::Pending)
    );
    assert_eq!(owner.judge().remaining_hazards(), 1);
    assert!(owner.judge().is_held(InputOwner {
        source: DeviceId(u64::MAX),
        physical: physical(),
        game_control: GameControlId(1)
    }));
    assert_eq!(owner.transport().anchors(), anchors);
    for command in accepted.audio_commands {
        assert_eq!(consumer.try_pop().unwrap(), command);
    }
    assert!(consumer.try_pop().is_err());
    let stop = AudioCommand::Stop {
        voice: VoiceId(1),
        at: ts(5000),
    };
    owner.enqueue_audio(stop).unwrap();
    assert_eq!(consumer.try_pop().unwrap(), stop);
    assert_eq!(owner.gameplay_fence(), Some(ts(10)));
    assert_eq!(owner.judge().stable_hash().unwrap(), hash);
}

#[test]
fn fenced_acquisition_retains_original_metadata_and_atomically_checks_clocks_sequences_and_reverse_mapping()
 {
    let (mut owner, mut consumer) = fixture(4);
    let mut unmapped = button(0, 999, ButtonState::Down);
    unmapped.meta_mut().clock_domain = ClockDomainId(77);
    assert!(matches!(
        owner.process_input(unmapped, &Clocks, point(2, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert_eq!(owner.fence_gameplay(), None);
    let mut unbound = button(10, 5, ButtonState::Down);
    if let PhysicalInputEvent::Button(event) = &mut unbound {
        event.control = PhysicalControlId::keyboard(99);
    }
    let first = owner.process_input(unbound, &Clocks, point(2, 0)).unwrap();
    assert!(first.bound_inputs.is_empty());
    assert_eq!(owner.judge().effective_song_time(), None);
    assert_eq!(owner.fence_gameplay(), Some(ts(10))); // Acquisition, not a fabricated judge result.
    let hash = owner.judge().stable_hash().unwrap();
    let mut original = button(20, 6, ButtonState::Down);
    original.meta_mut().clock_domain = ClockDomainId(9);
    original.meta_mut().timestamp = ts(20);
    let report = owner
        .process_input(original.clone(), &Clocks, point(8, 1000))
        .unwrap();
    frozen(&report, 10);
    assert_eq!(report.audio_at, point(2, 1500));
    let mut normalized = original.clone();
    normalized.meta_mut().clock_domain = ClockDomainId(1);
    normalized.meta_mut().timestamp = ts(1020);
    normalized.meta_mut().original_clock_point = Some(point(9, 20));
    assert_eq!(report.input, Some(normalized));
    assert_eq!(
        owner
            .process_input(button(21, 5, ButtonState::Up), &Clocks, point(2, 0))
            .unwrap_err(),
        RuntimeError::SequenceRegression {
            device: DeviceId(u64::MAX),
            last: 6,
            received: 5
        }
    );
    assert_eq!(
        owner
            .process_input(button(19, 99, ButtonState::Up), &Clocks, point(2, 0))
            .unwrap_err(),
        RuntimeError::NonMonotonicHost
    );
    assert!(matches!(
        owner.process_input(button(21, 99, ButtonState::Up), &Clocks, point(77, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    assert!(matches!(
        owner.advance_to(point(77, 2000), &Clocks, point(2, 0)),
        Err(RuntimeError::UnmappedClock { .. })
    ));
    frozen(
        &owner
            .process_input(button(21, 7, ButtonState::Up), &Clocks, point(2, 0))
            .unwrap(),
        10,
    );
    let retained_transport = owner.transport().clone();
    owner
        .transport_mut()
        .set_rate(ts(1021), Rate::new(-1, 1).unwrap())
        .unwrap();
    assert_eq!(
        owner
            .advance_to(point(1, 1022), &Clocks, point(2, 0))
            .unwrap_err(),
        RuntimeError::RequiresReplayRestore
    );
    let mut forward = retained_transport;
    owner.exchange_transport(&mut forward);
    owner.transport_mut().seek(ts(1021), ts(0)).unwrap();
    assert_eq!(
        owner
            .process_input(button(22, 99, ButtonState::Down), &Clocks, point(2, 0))
            .unwrap_err(),
        RuntimeError::RequiresReplayRestore
    );
    let mut restored_transport = transport();
    owner.exchange_transport(&mut restored_transport);
    frozen(
        &owner
            .process_input(button(22, 8, ButtonState::Down), &Clocks, point(2, 0))
            .unwrap(),
        10,
    );
    let (mut producer, mut alternate) = command_queue(2).unwrap();
    owner.exchange_audio_producer(&mut producer);
    assert_eq!(owner.gameplay_fence(), Some(ts(10)));
    assert_eq!(owner.judge().stable_hash().unwrap(), hash);
    let stop = AudioCommand::Stop {
        voice: VoiceId(u64::MAX),
        at: ts(-5),
    };
    owner.enqueue_audio(stop).unwrap();
    assert_eq!(alternate.try_pop().unwrap(), stop);
    assert!(consumer.try_pop().is_err());
}

#[test]
fn contact_routes_remain_owned_until_explicit_paired_restore_reopens_real_tail_and_hazard_processing()
 {
    let (mut owner, mut consumer) = fixture(8);
    let pristine = owner.judge().snapshot().unwrap();
    owner
        .configure_touch_router(
            TouchRouter::new(
                vec![TouchRegion {
                    device: DeviceSelector::Exact(DeviceId(u64::MAX)),
                    physical: physical(),
                    game_control: GameControlId(1),
                    min: Position2 { x: 0.0, y: 0.0 },
                    max: Position2 { x: 10.0, y: 10.0 },
                }],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    owner.set_song_end(ts(90)).unwrap();
    let down = owner
        .process_input_at(
            touch(10, 100, u64::MAX, TouchPhase::Down),
            Position2 { x: 2.0, y: 2.0 },
            &Clocks,
            point(2, 100),
        )
        .unwrap();
    assert_eq!(down.bound_inputs.len(), 1);
    assert_eq!(down.judge_events[0].stage, JudgeStage::HoldHead);
    let prefix = owner.judge().snapshot().unwrap();
    let router = owner.touch_router().unwrap().try_clone().unwrap();
    let hash = owner.judge().stable_hash().unwrap();
    assert_eq!(router.active_contacts(), 1);
    assert_eq!(owner.fence_gameplay(), Some(ts(10)));
    for (at, sequence, contact, phase) in [
        (20, 101, u64::MAX, TouchPhase::Move),
        (30, 102, 1, TouchPhase::Down),
        (40, 103, u64::MAX, TouchPhase::Cancel),
        (100, 104, u64::MAX, TouchPhase::Up),
    ] {
        let input = touch(at, sequence, contact, phase);
        let report = owner
            .process_input_at(
                input.clone(),
                Position2 { x: 2.0, y: 2.0 },
                &Clocks,
                point(2, at),
            )
            .unwrap();
        frozen(&report, 10);
        assert_eq!(report.input, Some(input));
        assert_eq!(owner.touch_router().unwrap().active_contacts(), 1);
        assert_eq!(owner.judge().stable_hash().unwrap(), hash);
    }
    let (_, _, old_router) = owner.replace_state_with_touch_router(
        JudgeEngine::from_snapshot(&prefix).unwrap(),
        transport(),
        Some(router),
    );
    assert_eq!(old_router.unwrap().active_contacts(), 1);
    assert_eq!(owner.gameplay_fence(), None);
    assert_eq!(owner.song_end(), None);
    let up = owner
        .process_input_at(
            touch(100, 1, u64::MAX, TouchPhase::Up),
            Position2 { x: 30.0, y: 2.0 },
            &Clocks,
            point(2, 300),
        )
        .unwrap();
    assert_eq!(up.bound_inputs[0].game_control, GameControlId(1));
    assert_eq!(up.judge_events[0].stage, JudgeStage::HoldTail);
    assert_eq!(up.hazard_events[0].id, HazardId(u64::MAX));
    assert_eq!(up.hazard_events[0].outcome, HazardOutcome::Triggered);
    assert_eq!(
        up.audio_commands,
        [play(2, 2, 300, 1.0), play(5, 5, 300, 0.25)]
    );
    assert_eq!(owner.touch_router().unwrap().active_contacts(), 0);
    for command in down.audio_commands.into_iter().chain(up.audio_commands) {
        assert_eq!(consumer.try_pop().unwrap(), command);
    }
    assert!(consumer.try_pop().is_err());
    assert_eq!(owner.fence_gameplay(), Some(ts(100)));
    owner.replace_state(JudgeEngine::from_snapshot(&pristine).unwrap(), transport());
    assert_eq!(owner.gameplay_fence(), None);
    assert_eq!(owner.fence_gameplay(), None);
    assert_eq!(owner.touch_router().unwrap().active_contacts(), 0);
    let accepted = owner
        .process_input_at(
            touch(10, 0, 1, TouchPhase::Down),
            Position2 { x: 2.0, y: 2.0 },
            &Clocks,
            point(2, 400),
        )
        .unwrap();
    assert_eq!(accepted.judge_events[0].stage, JudgeStage::HoldHead);
    owner.fence_gameplay();
    let (replacement, mut new_consumer) = command_queue(4).unwrap();
    owner.replace_session(
        JudgeEngine::from_snapshot(&pristine).unwrap(),
        transport(),
        replacement,
    );
    assert_eq!(owner.gameplay_fence(), None);
    assert_eq!(owner.fence_gameplay(), None);
    let fresh = owner
        .process_input(button(10, 0, ButtonState::Down), &Clocks, point(2, 500))
        .unwrap();
    for command in fresh.audio_commands {
        assert_eq!(new_consumer.try_pop().unwrap(), command);
    }
    for command in accepted.audio_commands {
        assert_eq!(consumer.try_pop().unwrap(), command);
    }
}

#[test]
fn finite_end_and_committed_audio_failure_latch_only_the_actual_frontier_without_retry_or_completion()
 {
    let (mut ended, mut empty_queue) = fixture(4);
    ended.set_song_end(ts(5)).unwrap();
    let report = ended
        .process_input(button(100, 1, ButtonState::Down), &Clocks, point(2, 0))
        .unwrap();
    assert!(report.song_end_reached);
    assert_eq!(report.song_time, ts(5));
    assert_eq!(ended.fence_gameplay(), Some(ts(5)));
    let hash = ended.judge().stable_hash().unwrap();
    let input = button(200, 2, ButtonState::Down);
    let later = ended
        .process_input(input.clone(), &Clocks, point(2, 300))
        .unwrap();
    frozen(&later, 5);
    assert_eq!(later.input, Some(input));
    assert!(later.song_end_reached);
    assert_eq!(ended.judge().remaining_hazards(), 2);
    assert_eq!(
        ended.judge().state(ObjectId(1)),
        Some(InteractionState::Pending)
    );
    assert_eq!(ended.judge().stable_hash().unwrap(), hash);
    assert!(empty_queue.try_pop().is_err());

    let (mut failed, mut consumer) = fixture(1);
    let sentinel = AudioCommand::Stop {
        voice: VoiceId(90),
        at: ts(600),
    };
    failed.enqueue_audio(sentinel).unwrap();
    let committed = failed
        .process_input(button(10, 1, ButtonState::Down), &Clocks, point(2, 700))
        .unwrap();
    assert_eq!(committed.judge_events.len(), 1);
    assert_eq!(committed.hazard_events.len(), 1);
    assert_eq!(committed.audio_failures.len(), 3);
    assert!(committed.audio_commands.is_empty());
    assert!(
        committed
            .audio_failures
            .iter()
            .all(|failure| failure.reason == QueuePushError::Full)
    );
    assert_eq!(failed.fence_gameplay(), Some(ts(10)));
    let hash = failed.judge().stable_hash().unwrap();
    assert_eq!(consumer.try_pop().unwrap(), sentinel);
    frozen(
        &failed
            .advance_to(point(1, 1100), &Clocks, point(2, 900))
            .unwrap(),
        10,
    );
    assert!(consumer.try_pop().is_err());
    assert_eq!(failed.judge().stable_hash().unwrap(), hash);
    failed.enqueue_audio(sentinel).unwrap();
    assert_eq!(consumer.try_pop().unwrap(), sentinel);
}
