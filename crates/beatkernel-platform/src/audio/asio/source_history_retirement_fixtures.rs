//! Portable real Mixer and ASIO planar PCM; no SDK/device authority.
use super::*;
use crate::audio::source_history_retirement_mailbox::SourceHistoryRetirementRequest;
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};

fn rig(capacity: usize) -> (CommandProducer, AsioBlockRenderer) {
    let format = AudioFormat::new(1000, 1).unwrap();
    let limits = PcmLimits::new(256, 256, 1).unwrap();
    let mut bank = SampleBank::new(format, limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 32], limits).unwrap(),
    )
    .unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        })
        .unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(1),
            Timestamp::ZERO,
            AudioLimits::new(8, 2, 8, 32, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.enable_source_playback_history(capacity).unwrap();
    (
        producer,
        AsioBlockRenderer::new(mixer, 2, vec![AsioPcmEncoding::Float32Lsb]).unwrap(),
    )
}

#[test]
fn actual_planar_pcm_and_reports_survive_repeated_callback_owned_compaction() {
    let (mut producer, mut renderer) = rig(3);
    let (mut reference_producer, mut reference) = rig(128);
    let mut completed = 0;
    for round in 0..32 {
        producer.request_pause(round % 2 == 1);
        reference_producer.request_pause(round % 2 == 1);
        let mut pcm = [0xa5; 8];
        let mut reference_pcm = [0xa5; 8];
        let report = renderer.render(&mut [&mut pcm]).unwrap();
        assert_eq!(report, reference.render(&mut [&mut reference_pcm]).unwrap());
        assert_eq!(pcm, reference_pcm);
        if round == 0 {
            assert_eq!(f32::from_le_bytes(pcm[..4].try_into().unwrap()), 0.25);
        }
        renderer
            .apply_source_history_retirement(
                SourceHistoryRetirementRequest {
                    revision: (round + 1) * 2,
                    source_pin: report.start_frame + report.frames as u64,
                },
                &mut completed,
            )
            .unwrap();
        assert_eq!(renderer.last_render_report(), Some(report));
        assert!(renderer.source_playback_history().unwrap().len() <= 2);
    }
}

#[test]
fn transferred_mixer_refuses_retirement_without_completing_request_or_changing_report() {
    let (_, mut renderer) = rig(4);
    let report = renderer.render(&mut [&mut [0; 8]]).unwrap();
    let mixer = renderer.take_mixer().unwrap();
    let mut completed = 0;
    assert_eq!(
        renderer.apply_source_history_retirement(
            SourceHistoryRetirementRequest {
                revision: 2,
                source_pin: 2,
            },
            &mut completed
        ),
        Err(AsioRenderError::MixerUnavailable)
    );
    assert_eq!(completed, 0);
    assert_eq!(renderer.last_render_report(), Some(report));
    assert_eq!(mixer.frame_cursor(), 2);
}

#[test]
fn destination_admission_refuses_short_planes_before_retirement_or_render() {
    let (_, mut renderer) = rig(4);
    let mut bad = [0xa5; 7];
    assert_eq!(
        renderer.validate_buffers(&[&mut bad]),
        Err(AsioRenderError::InvalidBuffers)
    );
    assert_eq!(
        renderer.render(&mut [&mut bad]),
        Err(AsioRenderError::InvalidBuffers)
    );
    assert_eq!(bad, [0xa5; 7]);
    assert_eq!(
        renderer
            .source_playback_history()
            .unwrap()
            .pulled_end_frame(),
        0
    );
    assert_eq!(renderer.last_render_report(), None);
}
