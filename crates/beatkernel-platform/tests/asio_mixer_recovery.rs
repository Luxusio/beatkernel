//! Deferred planar renderer ownership, without SDK/native callback retirement.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::asio::{AsioBlockRenderer, AsioPcmEncoding, AsioRenderError};
fn rig() -> (CommandProducer, AsioBlockRenderer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let pcm = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1., -0.25], pcm).unwrap(),
    )
    .unwrap();
    let (producer, consumer) = command_queue(8).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(7),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 4, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    (
        producer,
        AsioBlockRenderer::new(mixer, 2, vec![AsioPcmEncoding::Float32Lsb]).unwrap(),
    )
}
fn play(voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(1),
        at: Timestamp::from_nanos(at),
        gain,
    }
}
#[test]
fn take_after_planar_render_preserves_format_report_cursor_heads_and_unique_future_command_consumer()
 {
    let (mut producer, mut renderer) = rig();
    producer.try_push(play(u64::MAX, 0, 1.)).unwrap();
    let mut first = [0u8; 8];
    let report = renderer.render(&mut [&mut first]).unwrap();
    assert_eq!(f32::from_le_bytes(first[..4].try_into().unwrap()), 0.25);
    assert_eq!(f32::from_le_bytes(first[4..].try_into().unwrap()), 0.5);
    let format = renderer.format();
    let mut mixer = renderer.take_mixer().unwrap();
    assert_eq!(mixer.frame_cursor(), 2);
    assert!(renderer.take_mixer().is_none());
    assert_eq!(renderer.format(), format);
    assert_eq!(renderer.frames(), 2);
    assert_eq!(renderer.encodings(), [AsioPcmEncoding::Float32Lsb]);
    assert_eq!(renderer.last_render_report(), Some(report));
    producer
        .try_push(AudioCommand::Stop {
            voice: VoiceId(u64::MAX),
            at: Timestamp::from_nanos(750_000_000),
        })
        .unwrap();
    producer.try_push(play(7, 1_000_000_000, 0.5)).unwrap();
    let mut destination = [0xa5u8; 8];
    assert_eq!(
        renderer.render(&mut [&mut destination]),
        Err(AsioRenderError::MixerUnavailable)
    );
    assert_eq!(destination, [0xa5; 8]);
    assert_eq!(renderer.last_render_report(), Some(report));
    let mut pcm = [0.; 3];
    let continued = mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.75, 0., 0.125]);
    assert_eq!(continued.counters.commands_consumed, 3);
    assert_eq!(continued.counters.commands_applied, 3);
    assert_eq!(continued.counters.unknown_samples, 0);
}
#[test]
fn take_before_first_render_keeps_pristine_state_and_render_refusal_never_touches_any_plane() {
    let (mut producer, mut renderer) = rig();
    producer.try_push(play(7, 0, 0.5)).unwrap();
    let mut mixer = renderer.take_mixer().unwrap();
    assert_eq!(mixer.frame_cursor(), 0);
    assert_eq!(renderer.last_render_report(), None);
    let mut plane = [0x33u8; 8];
    assert_eq!(
        renderer.render(&mut [&mut plane]),
        Err(AsioRenderError::MixerUnavailable)
    );
    assert_eq!(plane, [0x33; 8]);
    assert!(renderer.take_mixer().is_none());
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.125, 0.25]);
    assert_eq!(mixer.counters().commands_consumed, 1);
}
