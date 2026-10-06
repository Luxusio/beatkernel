//! Deferred portable renderer ownership; no ASIO SDK/native callback evidence.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use beatkernel_platform::audio::asio::{AsioBlockRenderer, AsioPcmEncoding, AsioRenderError};
fn rig() -> (CommandProducer, Mixer) {
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
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(u32::MAX),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        )
        .with_playback_end_frame(8),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(7),
            sample: SampleId(1),
            at: Timestamp::from_nanos(1_500_000_000),
            gain: 1.,
        })
        .unwrap();
    (producer, mixer)
}
#[test]
fn renderer_validation_and_capacity_refusals_return_unchanged_original_mixer_and_legacy_error() {
    for (frames, encodings, expected) in [
        (
            0,
            vec![AsioPcmEncoding::Float32Lsb],
            AsioRenderError::InvalidConfiguration,
        ),
        (2, vec![], AsioRenderError::InvalidConfiguration),
        (
            2,
            vec![AsioPcmEncoding::Float32Lsb; 2],
            AsioRenderError::InvalidConfiguration,
        ),
        (
            9,
            vec![AsioPcmEncoding::Float32Lsb],
            AsioRenderError::Capacity,
        ),
        (
            u32::MAX,
            vec![AsioPcmEncoding::Float32Lsb],
            AsioRenderError::Capacity,
        ),
    ] {
        let (mut producer, mixer) = rig();
        let basis = mixer.output_frame_basis();
        let counters = mixer.counters();
        let failure = match AsioBlockRenderer::new_recoverable(mixer, frames, encodings.clone()) {
            Err(failure) => failure,
            Ok(_) => panic!("invalid setup must refuse"),
        };
        assert_eq!(*failure.error(), expected);
        assert_eq!(failure.mixer().unwrap().output_frame_basis(), basis);
        assert_eq!(failure.mixer().unwrap().counters(), counters);
        assert_eq!(
            (
                failure.mixer().unwrap().frame_cursor(),
                failure.mixer().unwrap().playback_frame_cursor(),
                failure.mixer().unwrap().is_paused()
            ),
            (5, 2, true)
        );
        let (_, legacy_mixer) = rig();
        let legacy = match AsioBlockRenderer::new(legacy_mixer, frames, encodings) {
            Err(error) => error,
            Ok(_) => panic!("legacy validation must refuse"),
        };
        assert_eq!(legacy, expected);
        let (_, mixer) = failure.into_parts();
        let mut mixer = mixer.unwrap();
        producer.request_pause(false);
        let mut pcm = [0.; 6];
        mixer.render(&mut pcm).unwrap();
        assert_eq!(pcm, [0.75, 1., 0., 0., 0.25, 0.5]);
    }
}
#[test]
fn successful_recoverable_renderer_preserves_paused_grid_pending_commands_pcm_and_finite_endpoint()
{
    let (mut producer, mixer) = rig();
    let format = mixer.config().format();
    let mut renderer =
        match AsioBlockRenderer::new_recoverable(mixer, 2, vec![AsioPcmEncoding::Float32Lsb]) {
            Ok(renderer) => renderer,
            Err(_) => panic!("valid setup required"),
        };
    assert_eq!(renderer.format(), format);
    assert_eq!(renderer.frames(), 2);
    assert!(renderer.last_render_report().is_none());
    let mut bytes = [0xa5; 8];
    let paused = renderer.render(&mut [&mut bytes]).unwrap();
    assert_eq!(bytes, [0; 8]);
    assert_eq!(
        (
            paused.start_frame,
            paused.playback_start_frame,
            paused.playback_frames
        ),
        (5, 2, 0)
    );
    producer.request_pause(false);
    for expected in [[0.75f32, 1.], [0., 0.], [0.25, 0.5]] {
        renderer.render(&mut [&mut bytes]).unwrap();
        assert_eq!(
            [
                f32::from_le_bytes(bytes[..4].try_into().unwrap()),
                f32::from_le_bytes(bytes[4..].try_into().unwrap())
            ],
            expected
        );
    }
    let report = renderer.last_render_report().unwrap();
    assert_eq!(report.playback_end_physical_frame, Some(13));
    let mixer = renderer.take_mixer().unwrap();
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (13, 8)
    );
    assert!(renderer.take_mixer().is_none());
    assert_eq!(renderer.last_render_report(), Some(report));
    bytes.fill(0xa5);
    assert_eq!(
        renderer.render(&mut [&mut bytes]),
        Err(AsioRenderError::MixerUnavailable)
    );
    assert_eq!(bytes, [0xa5; 8]);
}
