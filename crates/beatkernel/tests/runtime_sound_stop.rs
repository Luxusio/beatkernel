//! Deferred real Runtime/queue/software-Mixer fixtures; no output device is opened.
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig,
        PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId, VoiceId,
    },
    chart::*,
    input::*,
    interaction::PressInstantEvaluator,
    judge::*,
    runtime::{
        Runtime, RuntimeProcessingClock, SoundBinding,
        hazard_sound::{HazardSoundBinding, HazardSoundTimeline},
        input_sound::{InputSoundMarker, InputSoundTimeline},
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
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn transport() -> Transport {
    Transport::new(ts(1000), ts(0), Rate::NORMAL)
}
fn input(song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(u64::MAX), point(1, 1000 + song), sequence),
        control: PhysicalControlId::keyboard(4u16),
        state,
    })
}
fn judge(notes: bool, hazards: bool) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    let mut rules = Vec::new();
    if notes {
        for (id, at) in [(1, 0), (2, 100)] {
            source.objects.push(SourceObject {
                id: ObjectId(id),
                start: Beat::new(at).unwrap(),
                end: None,
                interaction: InteractionId(id as u32),
                visual: VisualId(0),
                audio: None,
                metadata: ObjectMetadata::default(),
            });
            rules.push(Rule {
                interaction: InteractionId(id as u32),
                control: GameControlId(1),
                evaluator: Box::new(PressInstantEvaluator),
            });
        }
    }
    let mut judge = JudgeEngine::new(
        source.compile().unwrap(),
        rules,
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
    if hazards {
        judge
            .configure_hazards(
                HazardTimeline::new(
                    vec![
                        HazardMarker {
                            id: HazardId(1),
                            at: ts(0),
                            control: GameControlId(1),
                            value: 7,
                        },
                        HazardMarker {
                            id: HazardId(2),
                            at: ts(0),
                            control: GameControlId(1),
                            value: u64::MAX,
                        },
                    ],
                    2,
                )
                .unwrap(),
            )
            .unwrap();
    }
    judge
}
fn runtime(
    judge: JudgeEngine,
    capacity: usize,
    controls: &[u32],
    sounds: Vec<SoundBinding>,
) -> (Runtime, CommandConsumer) {
    let bindings = BindingMap::from_bindings(controls.iter().map(|&control| Binding {
        device: DeviceSelector::Any,
        physical: PhysicalControlId::keyboard(4u16),
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
fn press(rows: &[(u32, u64)]) -> InputSoundTimeline {
    InputSoundTimeline::new(
        rows.iter()
            .map(|&(control, voice)| InputSoundMarker {
                control: GameControlId(control),
                at: ts(0),
                sample: SampleId(1),
                voice: VoiceId(voice),
                gain: 1.0,
            })
            .collect(),
        rows.len(),
    )
    .unwrap()
}
fn play(voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn stop(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(at),
    }
}

#[test]
fn prepared_unique_voice_union_stops_after_future_plays_without_stopping_external_bgm() {
    let normal = [(1, 9), (2, 5)]
        .map(|(object, voice)| SoundBinding {
            object: ObjectId(object),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(voice),
            gain: 1.0,
        })
        .to_vec();
    let (mut owner, consumer) = runtime(judge(true, true), 32, &[1, 2], normal);
    owner
        .configure_input_sounds(press(&[(1, 5), (2, 7)]))
        .unwrap();
    owner
        .configure_hazard_sounds(
            HazardSoundTimeline::new(
                vec![
                    HazardSoundBinding {
                        hazard: HazardId(1),
                        sample: SampleId(1),
                        voice: VoiceId(3),
                        gain: 1.0,
                    },
                    HazardSoundBinding {
                        hazard: HazardId(2),
                        sample: SampleId(1),
                        voice: VoiceId(9),
                        gain: 1.0,
                    },
                ],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    owner.enqueue_audio(play(99, 0, 0.125)).unwrap();
    let report = owner
        .process_input(
            input(0, 1, ButtonState::Down),
            &Identity,
            point(2, 1_000_000_000),
        )
        .unwrap();
    assert_eq!(
        report.audio_commands,
        [
            play(9, 1_000_000_000, 1.0),
            play(7, 1_000_000_000, 1.0),
            play(3, 1_000_000_000, 1.0),
            play(9, 1_000_000_000, 1.0)
        ]
    );
    assert_eq!(report.hazard_events.len(), 2);
    let hash = owner.judge().stable_hash().unwrap();
    assert_eq!(owner.fence_gameplay(), Some(ts(0)));
    let stopped = owner.fence_gameplay_sounds(ts(-100)).unwrap();
    assert_eq!(stopped.at, ts(1_000_000_000));
    assert_eq!(
        stopped.commands,
        [
            stop(3, 1_000_000_000),
            stop(5, 1_000_000_000),
            stop(7, 1_000_000_000),
            stop(9, 1_000_000_000)
        ]
    );
    assert!(stopped.failures.is_empty());
    assert!(owner.fence_gameplay_sounds(ts(i64::MAX)).is_none());
    assert_eq!(owner.judge().stable_hash().unwrap(), hash);
    let format = AudioFormat::new(10, 1).unwrap();
    let pcm = PcmLimits::new(128, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0; 32], pcm).unwrap(),
    )
    .unwrap();
    let limits = AudioLimits::new(32, 16, 32, 32, 32).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(format, ClockDomainId(2), ts(0), limits),
        bank,
        consumer,
    )
    .unwrap();
    let mut output = [0.0; 20];
    let rendered = mixer.render(&mut output).unwrap();
    // Before and after frame 10, only the separately owned BGM voice remains audible.
    assert_eq!(output, [0.125; 20]);
    assert_eq!(rendered.counters.unknown_stops, 1); // Configured voice 5 never played.
    assert_eq!(rendered.counters.commands_applied, 9);
    let after = owner
        .advance_to(point(1, 1100), &Identity, point(2, 2_000_000_000))
        .unwrap();
    assert!(after.audio_commands.is_empty() && after.hazard_events.is_empty());
}

#[test]
fn all_stops_are_attempted_once_and_rejected_future_plays_do_not_raise_the_watermark() {
    let (mut owner, mut consumer) = runtime(judge(false, false), 1, &[1], vec![]);
    owner
        .configure_input_sounds(press(&[(1, 3), (2, 7), (3, 9)]))
        .unwrap();
    let first = owner
        .process_input(input(0, 1, ButtonState::Down), &Identity, point(2, 10))
        .unwrap();
    assert_eq!(first.audio_commands, [play(3, 10, 1.0)]);
    owner
        .process_input(input(1, 2, ButtonState::Up), &Identity, point(2, 500))
        .unwrap();
    let rejected = owner
        .process_input(input(2, 3, ButtonState::Down), &Identity, point(2, 900))
        .unwrap();
    assert_eq!(rejected.audio_failures.len(), 1);
    assert_eq!(rejected.audio_failures[0].command, play(3, 900, 1.0));
    assert_eq!(rejected.audio_failures[0].reason, QueuePushError::Full);
    assert_eq!(consumer.try_pop().unwrap(), play(3, 10, 1.0));
    assert_eq!(owner.fence_gameplay(), Some(ts(2)));
    let stopped = owner.fence_gameplay_sounds(ts(20)).unwrap();
    assert_eq!(stopped.at, ts(20));
    assert_eq!(stopped.commands, [stop(3, 20)]);
    assert_eq!(
        stopped
            .failures
            .iter()
            .map(|error| error.command)
            .collect::<Vec<_>>(),
        [stop(7, 20), stop(9, 20)]
    );
    assert!(
        stopped
            .failures
            .iter()
            .all(|error| error.reason == QueuePushError::Full)
    );
    assert_eq!(owner.telemetry().counters().audio_commands, 2);
    assert_eq!(owner.telemetry().counters().queue_full, 3);
    assert_eq!(consumer.try_pop().unwrap(), stop(3, 20));
    assert!(owner.fence_gameplay_sounds(ts(900)).is_none());
    assert!(consumer.try_pop().is_err());
    // The same ordinary control producer remains usable; stopping is not a queue flush.
    owner.enqueue_audio(play(99, 30, 0.25)).unwrap();
    assert_eq!(consumer.try_pop().unwrap(), play(99, 30, 0.25));

    let (mut disconnected, consumer) = runtime(judge(false, false), 4, &[1], vec![]);
    disconnected
        .configure_input_sounds(press(&[(1, 3), (2, 7), (3, 9)]))
        .unwrap();
    disconnected
        .advance_to(point(1, 1000), &Identity, point(2, 0))
        .unwrap();
    disconnected.fence_gameplay();
    drop(consumer);
    let stopped = disconnected.fence_gameplay_sounds(ts(40)).unwrap();
    assert!(stopped.commands.is_empty());
    assert_eq!(stopped.failures.len(), 3);
    assert!(
        stopped
            .failures
            .iter()
            .all(|error| error.reason == QueuePushError::Disconnected)
    );
    assert_eq!(disconnected.telemetry().counters().queue_disconnected, 3);
    assert!(disconnected.fence_gameplay_sounds(ts(41)).is_none());
}

#[test]
fn unfenced_and_empty_calls_preserve_setup_and_real_restoration_resets_only_stop_state() {
    let initial = judge(false, false);
    let snapshot = initial.snapshot().unwrap();
    let (mut owner, mut consumer) = runtime(initial, 8, &[1], vec![]);
    assert!(owner.fence_gameplay_sounds(ts(9999)).is_none());
    assert_eq!(owner.fence_gameplay(), None);
    owner.configure_input_sounds(press(&[(1, 7)])).unwrap();
    owner
        .process_input(input(0, 1, ButtonState::Down), &Identity, point(2, 900))
        .unwrap();
    assert_eq!(consumer.try_pop().unwrap(), play(7, 900, 1.0));
    owner.fence_gameplay();
    assert_eq!(
        owner.fence_gameplay_sounds(ts(0)).unwrap().commands,
        [stop(7, 900)]
    );
    assert_eq!(consumer.try_pop().unwrap(), stop(7, 900));
    // Caller consumed the old output commands before replacing the paired judge/transport.
    owner.replace_state(JudgeEngine::from_snapshot(&snapshot).unwrap(), transport());
    assert_eq!(owner.gameplay_fence(), None);
    assert!(owner.fence_gameplay_sounds(ts(0)).is_none());
    owner
        .process_input(input(0, 1, ButtonState::Down), &Identity, point(2, 10))
        .unwrap();
    assert_eq!(consumer.try_pop().unwrap(), play(7, 10, 1.0));
    owner.fence_gameplay();
    let stopped = owner.fence_gameplay_sounds(ts(-1)).unwrap();
    assert_eq!(stopped.at, ts(10));
    assert_eq!(stopped.commands, [stop(7, 10)]);

    let (mut empty, consumer) = runtime(judge(false, false), 1, &[], vec![]);
    assert!(empty.fence_gameplay_sounds(ts(1)).is_none());
    empty
        .configure_hazard_sounds(HazardSoundTimeline::new(vec![], 0).unwrap())
        .unwrap();
    empty
        .advance_to(point(1, 1000), &Identity, point(2, 0))
        .unwrap();
    empty.fence_gameplay();
    let stopped = empty.fence_gameplay_sounds(ts(i64::MAX)).unwrap();
    assert_eq!(stopped.at, ts(i64::MAX));
    assert!(stopped.commands.is_empty() && stopped.failures.is_empty());
    assert!(empty.fence_gameplay_sounds(ts(i64::MIN)).is_none());
    drop(consumer);
}
