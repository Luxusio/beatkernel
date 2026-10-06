//! Actual core/native render paths with explicit same-rate channel conversion.
use super::*;
use beatkernel::audio::{
    AudioCommand, AudioFormat, AudioLimits, CommandProducer, MixerConfig, PcmLimits, PcmSample,
    SampleBank, SampleId, StoppedMixerSource, VoiceId,
};

fn rig(end: Option<u64>) -> (CommandProducer, Mixer) {
    let format = AudioFormat::new(48_000, 1).unwrap();
    let pcm = PcmLimits::new(64, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, pcm).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25, 0.5, 0.75, 1.], pcm).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = beatkernel::audio::command_queue(8).unwrap();
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
        AudioLimits::new(8, 2, 8, 128, 8).unwrap(),
    );
    if let Some(end) = end {
        config = config.with_playback_end_frame(end);
    }
    (producer, Mixer::new(config, bank, consumer).unwrap())
}
fn request() -> AlsaRequest {
    AlsaRequest {
        device: "null".into(),
        format: DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap(),
        buffer_frames: 256,
        period_frames: 64,
        allow_size_rounding: false,
        monotonic_domain: ClockDomainId(1),
    }
}

#[test]
fn same_rate_remix_preserves_source_reports_pause_and_finite_endpoint() {
    let (mut producer, mut mixer) = rig(Some(3));
    let basis = mixer.output_frame_basis();
    let matrix = ChannelMatrix::new(1, 2, &[1., 0.5]).unwrap();
    let mut remix = Some(
        FormatConverter::for_mixer(
            mixer.config(),
            request().format.pcm(),
            matrix,
            ResampleQuality::Linear,
            2,
        )
        .unwrap(),
    );
    let telemetry = Telemetry::new();
    let mut version = 0;
    let mut output = [0.; 4];
    let first = render_device_and_publish(
        &mut mixer,
        &mut remix,
        &mut output,
        &telemetry,
        &mut version,
    )
    .unwrap();
    assert_eq!(output, [0.25, 0.125, 0.5, 0.25]);
    assert_eq!(
        (first.start_frame, first.frames, first.playback_frames),
        (0, 2, 2)
    );
    assert_eq!(telemetry.read().render, Some(first));
    producer.request_pause(true);
    let pause = render_device_and_publish(
        &mut mixer,
        &mut remix,
        &mut output,
        &telemetry,
        &mut version,
    )
    .unwrap();
    assert_eq!(output, [0.; 4]);
    assert_eq!(
        (
            pause.start_frame,
            pause.playback_start_frame,
            pause.playback_frames
        ),
        (2, 2, 0)
    );
    assert!(pause.paused);
    assert_eq!(telemetry.read().render, Some(pause));
    producer.request_pause(false);
    let end = render_device_and_publish(
        &mut mixer,
        &mut remix,
        &mut output,
        &telemetry,
        &mut version,
    )
    .unwrap();
    assert_eq!(output, [0.75, 0.375, 0., 0.]);
    assert_eq!(
        (
            end.start_frame,
            end.playback_frames,
            end.playback_end_physical_frame
        ),
        (4, 1, Some(5))
    );
    assert!(end.paused);
    assert_eq!(telemetry.read().render, Some(end));
    assert_eq!(mixer.output_frame_basis().origin(), basis.origin());
    assert_eq!(remix.as_ref().unwrap().source_lookahead_frames(), 0);
    let next = render_device_and_publish(
        &mut mixer,
        &mut remix,
        &mut output,
        &telemetry,
        &mut version,
    )
    .unwrap();
    assert_eq!(output, [0.; 4]);
    assert_eq!(next.playback_end_physical_frame, Some(5));
}

#[test]
fn remix_preflight_rejects_rates_and_dimensions_and_returns_original_mixer() {
    for case in 0..3 {
        let (_producer, mut mixer) = rig(None);
        mixer.render(&mut [0.; 1]).unwrap();
        let before = mixer.output_frame_basis();
        let mut requested = request();
        let matrix = match case {
            0 => {
                requested.format =
                    DeviceFormat::new(44_100, 2, SampleEncoding::Float32, None).unwrap();
                ChannelMatrix::default_mix(1, 2).unwrap()
            }
            1 => ChannelMatrix::default_mix(2, 2).unwrap(),
            _ => ChannelMatrix::default_mix(1, 1).unwrap(),
        };
        let failure = AlsaStream::open_remixed_recoverable(requested, mixer, matrix)
            .err()
            .unwrap();
        let (error, original) = failure.into_parts();
        assert!(matches!(error, LinuxError::InvalidConfiguration(_)));
        let mut original = original.unwrap();
        assert_eq!(original.output_frame_basis(), before);
        let mut next = [0.];
        original.render(&mut next).unwrap();
        assert_eq!(next, [0.5]);
    }
}

#[test]
fn invalid_device_buffer_does_not_publish_or_advance_source() {
    let (_producer, mut mixer) = rig(None);
    let mut remix = Some(
        FormatConverter::for_mixer(
            mixer.config(),
            request().format.pcm(),
            ChannelMatrix::default_mix(1, 2).unwrap(),
            ResampleQuality::Linear,
            2,
        )
        .unwrap(),
    );
    let telemetry = Telemetry::new();
    let mut version = 0;
    assert_eq!(
        render_device_and_publish(
            &mut mixer,
            &mut remix,
            &mut [0.; 3],
            &telemetry,
            &mut version
        ),
        Err(AudioError::InvalidBuffer)
    );
    assert_eq!(mixer.frame_cursor(), 0);
    assert_eq!(version, 0);
    assert_eq!(telemetry.read().render, None);
}

#[test]
#[ignore = "explicit diagnostic requires the installed ALSA null plugin; no acoustic device"]
fn actual_null_plugin_remixes_and_retires_with_original_mixer() {
    let (_producer, mixer) = rig(Some(3));
    let basis = mixer.output_frame_basis();
    let mut stream = AlsaStream::open_remixed_recoverable(
        request(),
        mixer,
        ChannelMatrix::default_mix(1, 2).unwrap(),
    )
    .unwrap_or_else(|f| panic!("ALSA null open: {}", f.error()));
    assert_eq!(stream.frame_basis(), basis);
    assert_eq!(stream.configuration().format.channels(), 2);
    stream.start().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if stream
            .last_render_report()
            .is_some_and(|r| r.playback_end_physical_frame == Some(3))
            && stream.snapshot().submitted_frames >= 64
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "ALSA null report timeout: {:?}",
            stream.snapshot()
        );
        std::thread::yield_now();
    }
    stream.stop().unwrap();
    let original = stream.take_stopped_mixer().unwrap().unwrap();
    assert_eq!(original.config().format().channels(), 1);
    assert_eq!(original.playback_frame_cursor(), 3);
    assert_eq!(original.output_frame_basis().origin(), basis.origin());
}
