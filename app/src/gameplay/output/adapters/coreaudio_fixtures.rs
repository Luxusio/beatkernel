//! Deferred macOS-only failure mapping; no fabricated pending native stream.
use super::*;
use crate::gameplay_presentation_port_fixtures::{
    device, AudioCommand, VoiceId, SampleId, Timestamp, DeviceId,
};
#[test]
fn recovered_coreaudio_mapping_preserves_original_and_cleanup_errors_and_actual_paused_mixer() {
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
    let input: CoreAudioOpenFailure = OutputOpenFailure::recovered(
        CoreAudioError::Native {
            operation: "original registration",
            code: i32::MIN,
        },
        Some(mixer),
    )
    .with_cleanup_error(CoreAudioError::Native {
        operation: "original unregister",
        code: i32::MAX,
    });
    let mapped = map_open_failure(input, u64::MAX);
    assert!(matches!(
        mapped.error(),
        CoreAudioReplacementError::Native(CoreAudioError::Native {
            operation: "original registration",
            code: i32::MIN
        })
    ));
    assert!(matches!(
        mapped.cleanup_error(),
        Some(CoreAudioReplacementError::Native(CoreAudioError::Native {
            operation: "original unregister",
            code: i32::MAX
        }))
    ));
    assert!(mapped.pending_owner().is_none());
    assert_eq!(mapped.mixer().unwrap().output_frame_basis(), basis);
    assert_eq!(mapped.mixer().unwrap().counters(), counters);
    let (error, mixer, pending, cleanup) = mapped.into_parts();
    assert!(matches!(
        error,
        CoreAudioReplacementError::Native(CoreAudioError::Native { code: i32::MIN, .. })
    ));
    assert!(matches!(
        cleanup,
        Some(CoreAudioReplacementError::Native(CoreAudioError::Native {
            code: i32::MAX,
            ..
        }))
    ));
    assert!(pending.is_none());
    let mut mixer = mixer.unwrap();
    producer.request_pause(false);
    let mut pcm = [0.; 2];
    mixer.render(&mut pcm).unwrap();
    assert_eq!(pcm, [0.25, 0.5]);
}
#[test]
fn unavailable_coreaudio_mapping_retains_cleanup_diagnostic_without_constructing_replacement_owner()
{
    let input: CoreAudioOpenFailure =
        OutputOpenFailure::recovered(CoreAudioError::ConfigurationChanged, None)
            .with_cleanup_error(CoreAudioError::RecoveryUnavailable);
    let mapped = map_open_failure(input, 7);
    assert!(mapped.mixer().is_none());
    assert!(mapped.pending_owner().is_none());
    assert!(matches!(
        mapped.error(),
        CoreAudioReplacementError::Native(CoreAudioError::ConfigurationChanged)
    ));
    assert!(matches!(
        mapped.cleanup_error(),
        Some(CoreAudioReplacementError::Native(
            CoreAudioError::RecoveryUnavailable
        ))
    ));
    let (_, mixer, pending, cleanup) = mapped.into_parts();
    assert!(mixer.is_none());
    assert!(pending.is_none());
    assert!(cleanup.is_some());
}
