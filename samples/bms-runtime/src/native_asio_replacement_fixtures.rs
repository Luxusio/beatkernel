//! Deferred SDK-only mapping; no trusted driver request/control/stream is created.
use super::*;
use crate::gameplay_presentation_port_fixtures::{
    device, AudioCommand, VoiceId, SampleId, Timestamp, DeviceId,
};
use beatkernel_platform::windows::asio::control::{AsioControlError, AsioControlErrorDomain};
use beatkernel_platform::audio::asio::AsioRenderError;
fn model() -> (beatkernel::audio::CommandProducer, Mixer) {
    let (output, mut producer) = device(false, vec![DeviceId(u64::MAX)]);
    let mut mixer = output.mixer;
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    producer
        .try_push(AudioCommand::Play {
            voice: VoiceId(u64::MAX),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.,
        })
        .unwrap();
    (producer, mixer)
}
#[test]
fn actual_driver_open_error_mapping_retains_native_namespace_and_original_paused_mixer_without_control()
 {
    let (mut producer, mixer) = model();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let mapped = map_driver_open_error(
        AsioControlError::Native {
            operation: "original driver open",
            domain: AsioControlErrorDomain::Com,
            code: i32::MIN,
        },
        mixer,
    );
    assert!(matches!(
        mapped.error(),
        AsioReplacementError::Native(AsioStreamError::Control(AsioControlError::Native {
            operation: "original driver open",
            domain: AsioControlErrorDomain::Com,
            code: i32::MIN
        }))
    ));
    assert!(mapped.pending_owner().is_none());
    assert!(mapped.cleanup_error().is_none());
    assert_eq!(mapped.mixer().unwrap().output_frame_basis(), basis);
    assert_eq!(mapped.mixer().unwrap().counters(), counters);
    let (_, mixer, pending, cleanup) = mapped.into_parts();
    assert!(pending.is_none());
    assert!(cleanup.is_none());
    let mut mixer = mixer.unwrap();
    producer.request_pause(false);
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5]);
}
#[test]
fn recovered_or_unavailable_prepare_mapping_keeps_original_cleanup_and_never_fabricates_pending_native_stream()
 {
    for available in [false, true] {
        let (mut producer, mixer) = model();
        let basis = mixer.output_frame_basis();
        let failure: AsioPrepareFailure = OutputOpenFailure::recovered(
            AsioStreamError::Render(AsioRenderError::Capacity),
            available.then_some(mixer),
        )
        .with_cleanup_error(AsioStreamError::Control(AsioControlError::Native {
            operation: "original close",
            domain: AsioControlErrorDomain::Bridge,
            code: i32::MAX,
        }));
        let mapped = map_prepare_failure(failure, u64::MAX);
        assert!(matches!(
            mapped.error(),
            AsioReplacementError::Native(AsioStreamError::Render(AsioRenderError::Capacity))
        ));
        assert!(matches!(
            mapped.cleanup_error(),
            Some(AsioReplacementError::Native(AsioStreamError::Control(
                AsioControlError::Native {
                    operation: "original close",
                    domain: AsioControlErrorDomain::Bridge,
                    code: i32::MAX
                }
            )))
        ));
        assert!(mapped.pending_owner().is_none());
        assert_eq!(mapped.mixer().is_some(), available);
        let (_, mixer, pending, cleanup) = mapped.into_parts();
        assert!(pending.is_none());
        assert!(cleanup.is_some());
        if let Some(mut mixer) = mixer {
            assert_eq!(mixer.output_frame_basis(), basis);
            producer.request_pause(false);
            let mut pcm = [0.; 2];
            mixer.render(&mut pcm).unwrap();
            assert_eq!(pcm, [0.25, 0.5]);
        }
    }
}
