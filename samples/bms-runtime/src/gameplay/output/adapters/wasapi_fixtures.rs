//! Deferred Windows-only mapping, without WasapiStream or QpcClock construction.
use super::*;
use crate::gameplay_presentation_port_fixtures::{
    device, AudioCommand, VoiceId, SampleId, Timestamp, DeviceId,
};
#[test]
fn recovered_wasapi_error_mapping_keeps_native_code_and_unique_paused_mixer_with_original_producer()
{
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
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let mapped = map_open_failure(MixerOpenFailure::new(
        AudioPlatformError::Native { code: i32::MIN },
        Some(mixer),
    ));
    assert!(matches!(
        mapped.error(),
        WasapiReplacementError::Native(AudioPlatformError::Native { code: i32::MIN })
    ));
    assert!(mapped.pending_owner().is_none());
    assert!(mapped.cleanup_error().is_none());
    assert_eq!(mapped.mixer().unwrap().output_frame_basis(), basis);
    assert_eq!(mapped.mixer().unwrap().counters(), counters);
    let (error, mixer, pending, cleanup) = mapped.into_parts();
    assert!(matches!(
        error,
        WasapiReplacementError::Native(AudioPlatformError::Native { code: i32::MIN })
    ));
    assert!(pending.is_none());
    assert!(cleanup.is_none());
    let mut mixer = mixer.unwrap();
    producer.request_pause(false);
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5]);
}
#[test]
fn unavailable_wasapi_recovery_mapping_never_invents_stream_or_empty_mixer() {
    for (original, kind) in [
        (AudioPlatformError::WorkerFailure, 0),
        (AudioPlatformError::Native { code: i32::MAX }, 1),
        (AudioPlatformError::InvalidRequest, 2),
    ] {
        let mapped = map_open_failure(MixerOpenFailure::new(original, None));
        assert!(mapped.mixer().is_none());
        assert!(mapped.pending_owner().is_none());
        assert!(mapped.cleanup_error().is_none());
        assert!(matches!(
            (kind, mapped.error()),
            (
                0,
                WasapiReplacementError::Native(AudioPlatformError::WorkerFailure)
            ) | (
                1,
                WasapiReplacementError::Native(AudioPlatformError::Native { code: i32::MAX })
            ) | (
                2,
                WasapiReplacementError::Native(AudioPlatformError::InvalidRequest)
            )
        ));
        let (_, mixer, pending, cleanup) = mapped.into_parts();
        assert!(mixer.is_none());
        assert!(pending.is_none());
        assert!(cleanup.is_none());
    }
}
