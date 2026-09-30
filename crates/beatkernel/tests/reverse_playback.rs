use beatkernel::{
    audio::*,
    chart::*,
    input::*,
    interaction::InstantEvaluator,
    judge::*,
    replay::*,
    runtime::{playback::*, SoundBinding},
    time::*,
    transport::Rate,
};
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(9),
        timestamp: ts(n),
    }
}
fn fixture(queue: usize, policy: ReverseSoundPolicy) -> (ReversePlayback, Mixer) {
    let mut source = SourceChart::new(1000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(10).unwrap(),
        end: None,
        interaction: InteractionId(1),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    let engine = JudgeEngine::new(
        compile(&source).unwrap(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(InstantEvaluator),
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
    let mut replay = ReplaySession::new(
        ReplayHeader {
            version: REPLAY_VERSION,
            chart_identity: vec![1],
            rules_identity: vec![1],
            options: vec![],
            seed: 0,
            normalized_clock: ClockDomainId(1),
        },
        engine,
    )
    .unwrap();
    replay
        .push_input(
            GameInputEvent {
                game_control: GameControlId(1),
                physical: PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(
                        DeviceId(1),
                        ClockPoint {
                            domain: ClockDomainId(1),
                            timestamp: ts(10_000_000),
                        },
                        1,
                    ),
                    control: PhysicalControlId::keyboard(4),
                    state: ButtonState::Down,
                }),
            },
            ts(10_000_000),
        )
        .unwrap();
    replay.checkpoint().unwrap();
    ReversePlayback::new(
        replay,
        ts(10_000_000),
        vec![SoundBinding {
            object: ObjectId(1),
            stage: JudgeStage::Instant,
            sample: SampleId(1),
            voice: VoiceId(1),
            gain: 1.0,
        }],
        10,
        policy,
        Rate::NORMAL,
        config(queue, 0),
        bank(),
    )
    .unwrap()
}
fn config(queue: usize, origin: i64) -> MixerConfig {
    MixerConfig::new(
        AudioFormat::new(1000, 1).unwrap(),
        ClockDomainId(9),
        ts(origin),
        AudioLimits::new(queue, 4, 4, 32, queue).unwrap(),
    )
}
fn bank() -> SampleBank {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(1024, 4096, 4).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.1, 0.2, 0.3], limits).unwrap(),
    )
    .unwrap();
    bank
}
#[test]
fn literal_pcm_policies_restore_identical_logical_state() {
    for (policy, expected) in [
        (ReverseSoundPolicy::ReverseTimelineOnly, [0.1, 0.2, 0.3]),
        (ReverseSoundPolicy::ReverseSamples, [0.3, 0.2, 0.1]),
        (ReverseSoundPolicy::Mute, [0.0; 3]),
    ] {
        let (mut player, mut mixer) = fixture(4, policy);
        let before = player.replay().stable_hash().unwrap();
        let report = player.step_back(ts(0), point(0)).unwrap();
        assert_eq!(report.crossed.len(), 1);
        assert!(report.failed.is_empty());
        assert_eq!(
            report.admitted.len(),
            usize::from(policy != ReverseSoundPolicy::Mute)
        );
        let mut output = [0.0; 3];
        mixer.render(&mut output).unwrap();
        assert_eq!(output, expected);
        let zero = player.replay().stable_hash().unwrap();
        for _ in 0..3 {
            player.seek(ts(10_000_000)).unwrap();
            assert_eq!(player.replay().stable_hash().unwrap(), before);
            player.seek(ts(0)).unwrap();
            assert_eq!(player.replay().stable_hash().unwrap(), zero);
        }
    }
}
#[test]
fn mute_replacement_has_no_old_future_pending_commands() {
    let (mut player, mut old_mixer) = fixture(4, ReverseSoundPolicy::ReverseTimelineOnly);
    player.step_back(ts(0), point(100_000_000)).unwrap();
    old_mixer.render(&mut [0.0; 1]).unwrap(); // consumes future Play into old pending storage
    let hash = player.replay().stable_hash().unwrap();
    let (_, mut replacement) = player
        .transition(
            ReverseSoundPolicy::Mute,
            Rate::NORMAL,
            config(4, 110_000_000),
            bank(),
        )
        .unwrap();
    drop(old_mixer); // actual output owner disposal is explicit
    assert_eq!(player.replay().stable_hash().unwrap(), hash);
    player.seek(ts(10_000_000)).unwrap();
    let report = player.step_back(ts(0), point(110_000_000)).unwrap();
    assert!(report.admitted.is_empty());
    let mut output = [1.0; 4];
    let rendered = replacement.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 4]);
    assert_eq!(rendered.pending_commands, 0);
}
#[test]
fn queue_failure_does_not_change_reconstruction_or_retry() {
    let (mut full, mut mixer) = fixture(1, ReverseSoundPolicy::ReverseSamples);
    let report = full.step_back(ts(0), point(0)).unwrap(); // SetRate occupies sole slot
    assert_eq!(report.failed.len(), 1);
    assert!(report.admitted.is_empty());
    assert!(matches!(
        report.failed[0].command,
        AudioCommand::Play { .. }
    ));
    let (mut other, _) = fixture(4, ReverseSoundPolicy::Mute);
    other.step_back(ts(0), point(0)).unwrap();
    assert_eq!(
        full.replay().stable_hash().unwrap(),
        other.replay().stable_hash().unwrap()
    );
    let mut output = [1.0; 3];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.0; 3]);
}
#[test]
fn clocks_seek_and_checked_rate_are_explicit() {
    let (mut player, _) = fixture(4, ReverseSoundPolicy::Mute);
    let before = player.replay().stable_hash().unwrap();
    assert!(matches!(
        player.step_back(
            ts(0),
            ClockPoint {
                domain: ClockDomainId(1),
                timestamp: ts(0)
            }
        ),
        Err(ReversePlaybackError::OutputChronology)
    ));
    assert_eq!(player.replay().stable_hash().unwrap(), before);
    assert!(matches!(
        player.transition(
            ReverseSoundPolicy::ReverseSamples,
            Rate::new(i64::MIN, 1).unwrap(),
            config(4, 0),
            bank()
        ),
        Err(ReversePlaybackError::Overflow)
    ));
    player.observe_output_floor(point(1_000_000)).unwrap();
    assert!(matches!(
        player.step_back(ts(0), point(0)),
        Err(ReversePlaybackError::OutputChronology)
    ));
    let report = player.step_back(ts(0), point(1_000_000)).unwrap();
    assert_eq!(report.output_end, point(11_000_000));
    assert!(player
        .step_back(ts(0), point(11_000_000))
        .unwrap()
        .crossed
        .is_empty());
    player.seek(ts(10_000_000)).unwrap();
    assert_eq!(player.replay().stable_hash().unwrap(), before);
}

#[test]
fn normal_to_reverse_transition_changes_pcm_without_changing_judge() {
    let (mut player, mut mixer) = fixture(4, ReverseSoundPolicy::ReverseTimelineOnly);
    player.step_back(ts(0), point(0)).unwrap();
    let mut output = [0.0; 3];
    mixer.render(&mut output).unwrap();
    assert_eq!(output, [0.1, 0.2, 0.3]);
    player.seek(ts(10_000_000)).unwrap();
    let before = player.replay().stable_hash().unwrap();
    let (command, mut reverse) = player
        .transition(
            ReverseSoundPolicy::ReverseSamples,
            Rate::new(-1, 1).unwrap(),
            config(4, 10_000_000),
            bank(),
        )
        .unwrap();
    drop(mixer);
    assert!(matches!(
        command,
        AudioCommand::SetRate {
            rate: Rate::REVERSE,
            ..
        }
    ));
    assert_eq!(player.replay().stable_hash().unwrap(), before);
    player.step_back(ts(0), point(10_000_000)).unwrap();
    reverse.render(&mut output).unwrap();
    assert_eq!(output, [0.3, 0.2, 0.1]);
}
