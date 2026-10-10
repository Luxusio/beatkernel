//! Public Runtime/command-ring equivalence for immutable keysound fanout.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::PressInstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{
        input_sound::{InputSoundMarker, InputSoundTimeline},
        Runtime, RuntimeError, SoundBinding,
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
fn transport() -> Transport {
    Transport::new(ts(1000), ts(0), Rate::NORMAL)
}
fn input(song: i64, sequence: u64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(7), point(1, 1000 + song), sequence),
        control: PhysicalControlId::keyboard(4),
        state: ButtonState::Down,
    })
}
fn judge() -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = (1..=2)
        .map(|id| SourceObject {
            id: ObjectId(id),
            start: Beat::new(100).unwrap(),
            end: None,
            interaction: InteractionId(id as u32),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    JudgeEngine::new(
        source.compile().unwrap(),
        (1..=2)
            .map(|id| Rule {
                interaction: InteractionId(id),
                control: GameControlId(id),
                evaluator: Box::new(PressInstantEvaluator),
            })
            .collect(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}
fn make(
    sounds: Vec<SoundBinding>,
    capacity: usize,
    controls: &[u32],
) -> Result<(Runtime, CommandConsumer), RuntimeError> {
    make_judge(judge(), sounds, capacity, controls)
}
fn make_judge(
    judge: JudgeEngine,
    sounds: Vec<SoundBinding>,
    capacity: usize,
    controls: &[u32],
) -> Result<(Runtime, CommandConsumer), RuntimeError> {
    let bindings = BindingMap::from_bindings(controls.iter().map(|&control| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(control),
    }))
    .unwrap();
    let (producer, consumer) = command_queue(capacity).unwrap();
    Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        transport(),
        bindings,
        judge,
        producer,
        sounds,
        8,
    )
    .map(|runtime| (runtime, consumer))
}
fn sound(object: u64, stage: JudgeStage, voice: u64, gain: f32) -> SoundBinding {
    SoundBinding {
        object: ObjectId(object),
        stage,
        sample: SampleId(voice + 100),
        voice: VoiceId(voice),
        gain,
    }
}
fn fanout() -> Vec<SoundBinding> {
    vec![
        sound(2, JudgeStage::Instant, 21, 0.25),
        sound(1, JudgeStage::Instant, 11, -0.0),
        sound(1, JudgeStage::HoldHead, 91, 1.0),
        sound(2, JudgeStage::Instant, 22, -0.5),
        sound(1, JudgeStage::Instant, 12, 0.5),
        sound(1, JudgeStage::Instant, 11, -0.0),
    ]
}
fn signature(command: AudioCommand) -> (u64, u64, i64, u32) {
    match command {
        AudioCommand::Play {
            voice,
            sample,
            at,
            gain,
        } => (voice.0, sample.0, at.as_nanos(), gain.to_bits()),
        _ => panic!("expected keysound Play"),
    }
}
fn expected(at: i64) -> Vec<(u64, u64, i64, u32)> {
    [
        (11, -0.0_f32),
        (12, 0.5),
        (11, -0.0),
        (21, 0.25),
        (22, -0.5),
    ]
    .into_iter()
    .map(|(voice, gain)| (voice, voice + 100, at, gain.to_bits()))
    .collect()
}
fn drain(consumer: &mut CommandConsumer) -> Vec<AudioCommand> {
    let mut commands = Vec::new();
    while let Ok(command) = consumer.try_pop() {
        commands.push(command);
    }
    commands
}

#[test]
fn equal_key_duplicates_preserve_gain_bits_order_and_outer_event_order() {
    let (mut owner, mut consumer) = make(fanout(), 8, &[1, 2]).unwrap();
    let report = owner
        .process_input(input(100, 1), &NoMapping, point(2, -900))
        .unwrap();
    assert!(report.judge_error.is_none());
    assert_eq!(
        report
            .judge_events
            .iter()
            .map(|event| event.object.0)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(
        report
            .audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        expected(-900)
    );
    assert!(report.audio_failures.is_empty());
    assert_eq!(
        drain(&mut consumer)
            .into_iter()
            .map(signature)
            .collect::<Vec<_>>(),
        expected(-900)
    );
    let repeated = owner
        .process_input(input(100, 2), &NoMapping, point(2, 500))
        .unwrap();
    assert!(repeated.audio_commands.is_empty());
    assert!(drain(&mut consumer).is_empty());
}

#[test]
fn tiny_ring_preserves_exact_accepted_prefix_and_every_full_failure() {
    let (mut owner, mut consumer) = make(fanout(), 2, &[1, 2]).unwrap();
    let report = owner
        .process_input(input(100, 1), &NoMapping, point(2, 700))
        .unwrap();
    let all = expected(700);
    assert_eq!(
        report
            .audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        all[..2]
    );
    assert_eq!(
        report
            .audio_failures
            .iter()
            .map(|failure| signature(failure.command))
            .collect::<Vec<_>>(),
        all[2..]
    );
    assert!(report
        .audio_failures
        .iter()
        .all(|failure| failure.reason == QueuePushError::Full));
    assert_eq!(
        drain(&mut consumer)
            .into_iter()
            .map(signature)
            .collect::<Vec<_>>(),
        all[..2]
    );
    assert_eq!(owner.telemetry().counters().judge_results, 2);
    assert_eq!(owner.telemetry().counters().audio_commands, 2);
    assert_eq!(owner.telemetry().counters().queue_full, 3);
}

#[test]
fn disconnected_ring_reports_all_attempts_in_original_fanout_order() {
    let (mut owner, consumer) = make(fanout(), 8, &[1, 2]).unwrap();
    drop(consumer);
    let report = owner
        .process_input(input(100, 1), &NoMapping, point(2, 701))
        .unwrap();
    assert!(report.audio_commands.is_empty());
    assert_eq!(
        report
            .audio_failures
            .iter()
            .map(|failure| signature(failure.command))
            .collect::<Vec<_>>(),
        expected(701)
    );
    assert!(report
        .audio_failures
        .iter()
        .all(|failure| failure.reason == QueuePushError::Disconnected));
    assert_eq!(owner.telemetry().counters().queue_disconnected, 5);
}

fn fallback() -> InputSoundTimeline {
    InputSoundTimeline::new(
        vec![InputSoundMarker {
            control: GameControlId(3),
            at: ts(0),
            sample: SampleId(303),
            voice: VoiceId(203),
            gain: -0.25,
        }],
        1,
    )
    .unwrap()
}

#[test]
fn misses_emit_no_keysound_and_input_fallback_remains_after_all_hit_commands() {
    let (mut owner, mut consumer) = make(fanout(), 8, &[1, 2, 3]).unwrap();
    owner.configure_input_sounds(fallback()).unwrap();
    let report = owner
        .process_input(input(100, 1), &NoMapping, point(2, 702))
        .unwrap();
    let mut all = expected(702);
    all.push((203, 303, 702, (-0.25_f32).to_bits()));
    assert_eq!(
        report
            .audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        all
    );
    assert_eq!(
        drain(&mut consumer)
            .into_iter()
            .map(signature)
            .collect::<Vec<_>>(),
        all
    );
    let (mut missed, mut queue) = make(fanout(), 8, &[1, 2]).unwrap();
    let report = missed
        .advance_to(point(1, 1101), &NoMapping, point(2, 703))
        .unwrap();
    assert_eq!(report.judge_events.len(), 2);
    assert!(report.audio_commands.is_empty() && report.audio_failures.is_empty());
    assert!(drain(&mut queue).is_empty());
}

#[test]
fn replacement_restores_chronology_without_losing_fanout_and_rebinds_queue() {
    let (mut owner, mut first_queue) = make(fanout(), 8, &[1, 2]).unwrap();
    let snapshot = owner.judge().snapshot().unwrap();
    owner
        .process_input(input(100, 10), &NoMapping, point(2, 710))
        .unwrap();
    owner.replace_state(JudgeEngine::from_snapshot(&snapshot).unwrap(), transport());
    let restored = owner
        .process_input(input(100, 1), &NoMapping, point(2, 711))
        .unwrap();
    assert_eq!(
        restored
            .audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        expected(711)[..3]
    );
    assert_eq!(
        restored.audio_failures.len(),
        2,
        "restoration preserves already queued commands"
    );
    assert_eq!(drain(&mut first_queue).len(), 8);
    let (producer, mut replacement_queue) = command_queue(8).unwrap();
    let (_, _, old_producer) = owner.replace_session(
        JudgeEngine::from_snapshot(&snapshot).unwrap(),
        transport(),
        producer,
    );
    let report = owner
        .process_input(input(100, 0), &NoMapping, point(2, 712))
        .unwrap();
    assert_eq!(
        report
            .audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        expected(712)
    );
    assert_eq!(
        drain(&mut replacement_queue)
            .into_iter()
            .map(signature)
            .collect::<Vec<_>>(),
        expected(712)
    );
    assert!(drain(&mut first_queue).is_empty());
    drop(old_producer);
    assert!(first_queue.is_disconnected());
}

#[test]
fn invalid_gains_refuse_constructor_even_on_unrelated_stage_or_object() {
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut sounds = fanout();
        sounds.push(sound(u64::MAX, JudgeStage::HoldTail, 99, gain));
        assert!(matches!(
            make(sounds, 8, &[1, 2]),
            Err(RuntimeError::InvalidGain)
        ));
    }
}

#[test]
fn hundred_thousand_unrelated_bindings_do_not_change_selected_commands_or_stop_dedup() {
    let mut sounds = (0..100_000)
        .map(|index| sound(index + 10, JudgeStage::Instant, 99, 0.1))
        .collect::<Vec<_>>();
    sounds.insert(25_000, sound(1, JudgeStage::Instant, 11, -0.0));
    sounds.insert(75_000, sound(1, JudgeStage::Instant, 12, 0.5));
    sounds.push(sound(1, JudgeStage::Instant, 11, -0.0));
    let (mut owner, mut consumer) = make(sounds, 8, &[1]).unwrap();
    let report = owner
        .process_input(input(100, 1), &NoMapping, point(2, 713))
        .unwrap();
    assert_eq!(
        report
            .audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        expected(713)[..3]
    );
    assert!(report.audio_failures.is_empty());
    drain(&mut consumer);
    assert!(owner.fence_gameplay().is_some());
    let stopped = owner.fence_gameplay_sounds(ts(714)).unwrap();
    assert_eq!(
        stopped.commands,
        [
            AudioCommand::Stop {
                voice: VoiceId(11),
                at: ts(714)
            },
            AudioCommand::Stop {
                voice: VoiceId(12),
                at: ts(714)
            },
            AudioCommand::Stop {
                voice: VoiceId(99),
                at: ts(714)
            }
        ]
    );
    assert!(stopped.failures.is_empty());
    assert_eq!(drain(&mut consumer), stopped.commands);
}

struct OrderedCustom;
struct CustomInteraction {
    control: GameControlId,
    state: beatkernel::interaction::InteractionState,
}
impl beatkernel::interaction::InteractionEvaluator for OrderedCustom {
    fn validate(
        &self,
        _: &beatkernel::chart::TimedObject,
        _: &JudgeProfile,
    ) -> Result<(), beatkernel::judge::JudgeError> {
        Ok(())
    }
    fn begin(
        &self,
        _: &beatkernel::chart::TimedObject,
        context: &beatkernel::interaction::BeginContext<'_>,
    ) -> Box<dyn beatkernel::interaction::ActiveInteraction> {
        Box::new(CustomInteraction {
            control: context.control,
            state: beatkernel::interaction::InteractionState::Pending,
        })
    }
}
impl beatkernel::interaction::ActiveInteraction for CustomInteraction {
    fn state(&self) -> beatkernel::interaction::InteractionState {
        self.state
    }
    fn accepts_input(
        &self,
        event: &beatkernel::input::GameInputEvent,
        _: &beatkernel::interaction::InteractionContext<'_>,
    ) -> bool {
        self.state == beatkernel::interaction::InteractionState::Pending
            && event.game_control == self.control
            && matches!(
                event.physical,
                PhysicalInputEvent::Button(ButtonEvent {
                    state: ButtonState::Down,
                    ..
                })
            )
    }
    fn on_input(
        &mut self,
        _: &beatkernel::input::GameInputEvent,
        _: &beatkernel::interaction::InteractionContext<'_>,
    ) -> beatkernel::interaction::InteractionOutput {
        self.state = beatkernel::interaction::InteractionState::Completed;
        beatkernel::interaction::InteractionOutput {
            results: [77, 3]
                .map(|stage| beatkernel::interaction::InteractionResult {
                    stage: JudgeStage::Custom(stage),
                    outcome: beatkernel::judge::JudgeOutcome::Hit {
                        grade: JudgeGrade(9),
                        delta: Duration::ZERO,
                    },
                })
                .to_vec(),
        }
    }
    fn advance_to(
        &mut self,
        _: Timestamp,
        _: &beatkernel::interaction::InteractionContext<'_>,
    ) -> beatkernel::interaction::InteractionOutput {
        Default::default()
    }
    fn deadline(&self, _: &JudgeProfile) -> Option<i128> {
        None
    }
}

#[test]
fn actual_hold_head_tail_and_custom_stages_keep_each_fanout_and_callback_order() {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = [
        SourceObject {
            id: ObjectId(1),
            start: Beat::new(100).unwrap(),
            end: Some(Beat::new(200).unwrap()),
            interaction: InteractionId(1),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        },
        SourceObject {
            id: ObjectId(2),
            start: Beat::new(100).unwrap(),
            end: None,
            interaction: InteractionId(2),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        },
    ]
    .to_vec();
    let judge = JudgeEngine::new(
        source.compile().unwrap(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(beatkernel::interaction::PressHoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(2),
                evaluator: Box::new(OrderedCustom),
            },
        ],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    let sounds = vec![
        sound(1, JudgeStage::HoldTail, 21, 0.5),
        sound(2, JudgeStage::Custom(3), 51, -0.5),
        sound(2, JudgeStage::Custom(77), 41, -0.0),
        sound(1, JudgeStage::HoldHead, 31, 1.0),
        sound(2, JudgeStage::Custom(77), 42, 0.25),
        sound(1, JudgeStage::HoldTail, 22, -0.25),
        sound(1, JudgeStage::Instant, 91, 1.0),
        sound(2, JudgeStage::Custom(78), 92, 1.0),
    ];
    let (mut owner, mut consumer) = make_judge(judge, sounds, 8, &[1, 2]).unwrap();
    let head = owner
        .process_input(input(100, 1), &NoMapping, point(2, 900))
        .unwrap();
    assert_eq!(
        head.judge_events
            .iter()
            .map(|event| (event.object.0, event.stage))
            .collect::<Vec<_>>(),
        [
            (1, JudgeStage::HoldHead),
            (2, JudgeStage::Custom(77)),
            (2, JudgeStage::Custom(3))
        ]
    );
    let wanted = [(31, 1.0_f32), (41, -0.0), (42, 0.25), (51, -0.5)]
        .map(|(voice, gain)| (voice, voice + 100, 900, gain.to_bits()));
    assert_eq!(
        head.audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        wanted
    );
    assert_eq!(
        drain(&mut consumer)
            .into_iter()
            .map(signature)
            .collect::<Vec<_>>(),
        wanted
    );
    let mut release = input(200, 2);
    if let PhysicalInputEvent::Button(button) = &mut release {
        button.state = ButtonState::Up;
    }
    let tail = owner
        .process_input(release, &NoMapping, point(2, 901))
        .unwrap();
    assert_eq!(tail.judge_events.len(), 1);
    assert_eq!(tail.judge_events[0].stage, JudgeStage::HoldTail);
    let wanted =
        [(21, 0.5_f32), (22, -0.25)].map(|(voice, gain)| (voice, voice + 100, 901, gain.to_bits()));
    assert_eq!(
        tail.audio_commands
            .iter()
            .copied()
            .map(signature)
            .collect::<Vec<_>>(),
        wanted
    );
    assert_eq!(
        drain(&mut consumer)
            .into_iter()
            .map(signature)
            .collect::<Vec<_>>(),
        wanted
    );
}
