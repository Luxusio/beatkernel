//! Finite replay traversal and literal keysound rendering; no native device.
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
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for policy in [
        ReverseSoundPolicy::ReverseTimelineOnly,
        ReverseSoundPolicy::ReverseSamples,
        ReverseSoundPolicy::Mute,
    ] {
        let (mut player, mut mixer) = fixture(4, policy);
        let original = player.replay().stable_hash()?;
        let traversal = player.step_back(ts(0), point(0))?;
        let mut pcm = [0.0; 4];
        let rendered = mixer.render(&mut pcm)?;
        player.seek(ts(10_000_000))?;
        println!(
            "{policy:?}: crossed={} admitted={} failed={} PCM={pcm:?} applied={} restored={}",
            traversal.crossed.len(),
            traversal.admitted.len(),
            traversal.failed.len(),
            rendered.counters.commands_applied,
            player.replay().stable_hash()? == original
        );
    }
    Ok(())
}
