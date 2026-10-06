//! SDK-free actual Mixer delivery with explicit native target channels.
use beatkernel::{
    audio::{
        AudioCommand, AudioFormat, AudioLimits, ChannelMatrix, CommandProducer, Mixer, MixerConfig,
        PcmLimits, PcmSample, SampleBank, SampleId, VoiceId, command_queue,
    },
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::asio::{AsioBlockRenderer, AsioPcmEncoding::*, AsioRenderError};

fn rig(end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(4, 1).unwrap();
    let limits = PcmLimits::new(64, 128, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mut config = MixerConfig::new(
        format,
        ClockDomainId(9),
        Timestamp::ZERO,
        AudioLimits::new(8, 2, 8, 2, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}

#[test]
fn remixed_heterogeneous_planes_keep_pause_finite_endpoint_and_original_mixer() {
    let (mut producer, mixer) = rig(Some(3));
    let source = mixer.config().format();
    let target = AudioFormat::new(4, 2).unwrap();
    let mut renderer = AsioBlockRenderer::new_remixed_recoverable(
        mixer,
        2,
        target,
        vec![Int16Lsb, Float32Msb],
        ChannelMatrix::new(1, 2, &[1., 0.5]).unwrap(),
    )
    .unwrap_or_else(|f| panic!("{}", f.error()));
    assert_eq!(renderer.format(), target);
    assert_eq!(renderer.source_format(), source);
    let mut left = [0xa5; 4];
    let mut right = [0xa5; 8];
    let first = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(left, [0, 32, 0, 64]);
    assert_eq!(right, [0x3e, 0, 0, 0, 0x3e, 0x80, 0, 0]);
    assert_eq!(
        (first.start_frame, first.frames, first.playback_frames),
        (0, 2, 2)
    );
    producer.request_pause(true);
    let pause = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert!(pause.paused);
    assert_eq!((pause.start_frame, pause.playback_frames), (2, 0));
    assert_eq!(left, [0; 4]);
    assert_eq!(right, [0; 8]);
    producer.request_pause(false);
    let end = renderer.render(&mut [&mut left, &mut right]).unwrap();
    assert_eq!(left, [0, 96, 0, 0]);
    assert_eq!(right, [0x3e, 0xc0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(end.playback_end_physical_frame, Some(5));
    assert_eq!(end.playback_frames, 1);
    let original = renderer.take_mixer().unwrap();
    assert_eq!(original.config().format(), source);
    assert_eq!(original.playback_frame_cursor(), 3);
    assert_eq!(renderer.source_format(), source);
    left.fill(0xa5);
    right.fill(0xa5);
    assert_eq!(
        renderer.render(&mut [&mut left, &mut right]),
        Err(AsioRenderError::MixerUnavailable)
    );
    assert_eq!(left, [0xa5; 4]);
    assert_eq!(right, [0xa5; 8]);
}

#[test]
fn remixed_invalid_planes_leave_source_progress_and_destinations_unchanged() {
    let (_producer, mixer) = rig(None);
    let mut renderer = AsioBlockRenderer::new_remixed_recoverable(
        mixer,
        2,
        AudioFormat::new(4, 2).unwrap(),
        vec![Int16Lsb, Float32Msb],
        ChannelMatrix::default_mix(1, 2).unwrap(),
    )
    .unwrap_or_else(|f| panic!("{}", f.error()));
    let mut left = [0xa5; 4];
    let mut bad = [0xa5; 7];
    assert_eq!(
        renderer.render(&mut [&mut left, &mut bad]),
        Err(AsioRenderError::InvalidBuffers)
    );
    assert_eq!(left, [0xa5; 4]);
    assert_eq!(bad, [0xa5; 7]);
    assert_eq!(renderer.last_render_report(), None);
    let original = renderer.take_mixer().unwrap();
    assert_eq!(original.frame_cursor(), 0);
    assert_eq!(original.counters().commands_applied, 0);
}

#[test]
fn remixed_setup_refusals_return_source_with_unconsumed_audio() {
    for case in 0..5 {
        let (_producer, mixer) = rig(None);
        let mut target = AudioFormat::new(4, 2).unwrap();
        let mut frames = 2;
        let mut encodings = vec![Int16Lsb, Float32Msb];
        let mut matrix = ChannelMatrix::default_mix(1, 2).unwrap();
        match case {
            0 => target = AudioFormat::new(8, 2).unwrap(),
            1 => {
                encodings.pop();
            }
            2 => matrix = ChannelMatrix::default_mix(2, 2).unwrap(),
            3 => frames = 0,
            _ => frames = 3,
        }
        let failure =
            AsioBlockRenderer::new_remixed_recoverable(mixer, frames, target, encodings, matrix)
                .err()
                .unwrap();
        let mut original = failure.into_parts().1.unwrap();
        assert_eq!(original.frame_cursor(), 0);
        let mut output = [0.];
        original.render(&mut output).unwrap();
        assert_eq!(output, [0.25]);
    }
}
