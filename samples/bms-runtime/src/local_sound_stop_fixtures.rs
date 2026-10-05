//! Deferred exact-member control on the real shared queue; no hardware output.
use crate::{
    local_players::PlayerId,
    local_runtime::{InputResult, MemberConfig, RuntimeGroup},
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandConsumer, Mixer, MixerConfig,
        PcmLimits, PcmSample, QueuePushError, SampleBank, SampleId, VoiceId,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        GameControlId, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::PressInstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{RuntimeProcessingClock, SoundBinding},
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
fn player(index: usize) -> PlayerId {
    PlayerId([7, u32::MAX][index])
}
fn input(index: usize, song: i64, sequence: u64, state: ButtonState) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(
            DeviceId(u64::MAX - index as u64),
            point(1, 1000 + song),
            sequence,
        ),
        control: PhysicalControlId::keyboard(4u16),
        state,
    })
}
fn group(capacity: usize) -> (RuntimeGroup, CommandConsumer) {
    let configs = (0..2)
        .map(|index| {
            let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
            for (id, at) in [(1, 0), (2, 2_000_000_000)] {
                source.objects.push(SourceObject {
                    id: ObjectId(id),
                    start: Beat::new(at).unwrap(),
                    end: None,
                    interaction: InteractionId(1),
                    visual: VisualId(0),
                    audio: None,
                    metadata: ObjectMetadata::default(),
                });
            }
            let judge = JudgeEngine::new(
                source.compile().unwrap(),
                vec![Rule {
                    interaction: InteractionId(1),
                    control: GameControlId(1),
                    evaluator: Box::new(PressInstantEvaluator),
                }],
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
            .unwrap();
            let device = DeviceId(u64::MAX - index as u64);
            MemberConfig {
                player: player(index),
                device: Some(device),
                judge,
                bindings: BindingMap::from_bindings([Binding {
                    device: DeviceSelector::Exact(device),
                    physical: PhysicalControlId::keyboard(4u16),
                    game_control: GameControlId(1),
                }])
                .unwrap(),
                sounds: [1, 2]
                    .map(|id| SoundBinding {
                        object: ObjectId(id),
                        stage: JudgeStage::Instant,
                        sample: SampleId(1),
                        voice: VoiceId([11, 22][index]),
                        gain: 0.25,
                    })
                    .to_vec(),
            }
        })
        .collect();
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(1000), ts(0), Rate::NORMAL),
        producer,
        configs,
        8,
        &[VoiceId(99)],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer)
}
fn stop(voice: u64, at: i64) -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(voice),
        at: ts(at),
    }
}
fn bgm() -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(1),
        voice: VoiceId(99),
        at: ts(0),
        gain: 0.125,
    }
}

#[test]
fn exact_member_stops_preserve_other_players_bgm_and_poisoned_control_ownership() {
    let (mut owner, consumer) = group(16);
    assert!(owner.fence_player_sounds(PlayerId(0), ts(0)).is_err());
    assert!(
        owner
            .fence_player_sounds(player(0), ts(0))
            .unwrap()
            .is_none()
    );
    owner.enqueue_audio(bgm()).unwrap();
    for index in 0..2 {
        let InputResult::Processed(reports) = owner
            .process_input(
                input(index, 0, 1, ButtonState::Down),
                &Identity,
                point(2, 0),
            )
            .unwrap()
        else {
            panic!("assigned full-width source")
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].player, player(index));
        assert_eq!(reports[0].report.judge_events.len(), 1);
    }
    let survivor_hash = owner
        .member_judge(player(1))
        .unwrap()
        .stable_hash()
        .unwrap();
    assert_eq!(owner.fence_player(player(0)).unwrap(), Some(ts(0)));
    let stopped = owner
        .fence_player_sounds(player(0), ts(1_000_000_000))
        .unwrap()
        .unwrap();
    assert_eq!(stopped.commands, [stop(11, 1_000_000_000)]);
    assert!(stopped.failures.is_empty());
    assert!(
        owner
            .fence_player_sounds(player(0), ts(0))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        owner
            .member_judge(player(1))
            .unwrap()
            .stable_hash()
            .unwrap(),
        survivor_hash
    );
    assert_eq!(owner.player_gameplay_fence(player(1)), None);
    let format = AudioFormat::new(10, 1).unwrap();
    let pcm = PcmLimits::new(160, 160, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![1.0; 40], pcm).unwrap(),
    )
    .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            ts(0),
            AudioLimits::new(16, 4, 16, 20, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut output = [0.0; 20];
    let rendered = mixer.render(&mut output).unwrap();
    assert_eq!(&output[..10], &[0.625; 10]);
    assert_eq!(&output[10..], &[0.375; 10]);
    assert_eq!(rendered.counters.unknown_stops, 0);
    owner
        .process_input(
            input(1, 1_000_000_000, 2, ButtonState::Up),
            &Identity,
            point(2, 1_000_000_000),
        )
        .unwrap();
    let InputResult::Processed(reports) = owner
        .process_input(
            input(1, 2_000_000_000, 3, ButtonState::Down),
            &Identity,
            point(2, 2_000_000_000),
        )
        .unwrap()
    else {
        panic!("survivor remains live")
    };
    assert_eq!(reports[0].report.judge_events.len(), 1);
    assert!(matches!(
        reports[0].report.audio_commands.as_slice(),
        [AudioCommand::Play {
            voice: VoiceId(22),
            ..
        }]
    ));
    assert!(!owner.poisoned());

    let (mut poisoned, mut consumer) = group(1);
    poisoned
        .process_input(input(0, 0, 1, ButtonState::Down), &Identity, point(2, 5))
        .unwrap();
    let error = poisoned
        .process_input(input(1, 0, 1, ButtonState::Down), &Identity, point(2, 900))
        .unwrap_err();
    assert_eq!(error.completed_reports.len(), 1);
    assert_eq!(error.completed_reports[0].player, player(1));
    assert_eq!(
        error.completed_reports[0].report.audio_failures[0].reason,
        QueuePushError::Full
    );
    assert!(poisoned.poisoned());
    assert_eq!(poisoned.fence_player(player(0)).unwrap(), Some(ts(0)));
    let rejected = poisoned
        .fence_player_sounds(player(0), ts(6))
        .unwrap()
        .unwrap();
    assert!(rejected.commands.is_empty());
    assert_eq!(rejected.failures[0].command, stop(11, 6));
    assert_eq!(rejected.failures[0].reason, QueuePushError::Full);
    assert!(matches!(
        consumer.try_pop().unwrap(),
        AudioCommand::Play {
            voice: VoiceId(11),
            ..
        }
    ));
    assert!(poisoned.fence_player_sounds(PlayerId(0), ts(0)).is_err());
    assert!(consumer.try_pop().is_err());
    assert_eq!(poisoned.fence_player(player(1)).unwrap(), Some(ts(0)));
    let stopped = poisoned
        .fence_player_sounds(player(1), ts(8))
        .unwrap()
        .unwrap();
    // Failed Play at 900 never admitted a future scheduling obligation.
    assert_eq!(stopped.commands, [stop(22, 8)]);
    assert_eq!(consumer.try_pop().unwrap(), stop(22, 8));
    assert!(
        poisoned
            .fence_player_sounds(player(0), ts(7))
            .unwrap()
            .is_none()
    );
    assert!(poisoned.poisoned());
    poisoned.enqueue_audio(bgm()).unwrap();
    assert_eq!(consumer.try_pop().unwrap(), bgm());
}
