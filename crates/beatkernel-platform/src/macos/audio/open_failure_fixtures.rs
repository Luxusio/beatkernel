//! Deferred macOS-only pure preflight; no MachClock/native acquisition.
use super::*;
use beatkernel::audio::{command_queue, AudioLimits, MixerConfig, PcmLimits, SampleBank};
fn mixer() -> Mixer {
    let format = AudioFormat::new(48_000, 2).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 128, 2).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(u32::MAX),
            Timestamp::from_nanos(-123),
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 4]).unwrap();
    mixer
}
#[test]
fn pure_coreaudio_preflight_rejects_invalid_device_buffer_format_and_capacity_before_native_ownership(
) {
    let mixer = mixer();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let valid = CoreAudioRequest {
        device: u32::MAX,
        format: AudioFormat::new(48_000, 2).unwrap(),
        buffer_frames: 8,
    };
    for request in [
        CoreAudioRequest { device: 0, ..valid },
        CoreAudioRequest {
            buffer_frames: 0,
            ..valid
        },
        CoreAudioRequest {
            format: AudioFormat::new(44_100, 2).unwrap(),
            ..valid
        },
        CoreAudioRequest {
            format: AudioFormat::new(48_000, 1).unwrap(),
            ..valid
        },
    ] {
        assert_eq!(
            validate_open(&request, &mixer),
            Err(CoreAudioError::InvalidRequest)
        );
    }
    assert_eq!(
        validate_open(
            &CoreAudioRequest {
                buffer_frames: 9,
                ..valid
            },
            &mixer
        ),
        Err(CoreAudioError::Capacity)
    );
    assert_eq!(mixer.output_frame_basis(), basis);
    assert_eq!(mixer.counters(), counters);
    assert_eq!(mixer.frame_cursor(), 2);
}
#[test]
fn pure_coreaudio_exact_capacity_boundary_accepts_without_resolving_device_or_constructing_mach_clock(
) {
    let mixer = mixer();
    let before = mixer.output_frame_basis();
    for device in [1, u32::MAX] {
        for buffer_frames in [1, 8] {
            assert_eq!(
                validate_open(
                    &CoreAudioRequest {
                        device,
                        format: AudioFormat::new(48_000, 2).unwrap(),
                        buffer_frames
                    },
                    &mixer
                ),
                Ok(())
            );
        }
    }
    assert_eq!(mixer.output_frame_basis(), before);
}

#[test]
fn pure_coreaudio_remix_preflight_checks_target_layout_rate_and_preserves_source_basis() {
    let mixer = mixer();
    let basis = mixer.output_frame_basis();
    let request = CoreAudioRequest {
        device: u32::MAX,
        format: AudioFormat::new(48_000, 1).unwrap(),
        buffer_frames: 8,
    };
    let matrix = ChannelMatrix::default_mix(2, 1).unwrap();
    assert_eq!(
        validate_open_with_matrix(&request, &mixer, Some(&matrix)),
        Ok(())
    );
    assert_eq!(
        validate_open(&request, &mixer),
        Err(CoreAudioError::InvalidRequest)
    );
    assert_eq!(
        validate_open_with_matrix(
            &request,
            &mixer,
            Some(&ChannelMatrix::default_mix(1, 1).unwrap())
        ),
        Err(CoreAudioError::InvalidRequest)
    );
    assert_eq!(
        validate_open_with_matrix(
            &CoreAudioRequest {
                format: AudioFormat::new(44_100, 1).unwrap(),
                ..request
            },
            &mixer,
            Some(&matrix)
        ),
        Err(CoreAudioError::InvalidRequest)
    );
    assert_eq!(
        validate_open_with_matrix(
            &CoreAudioRequest {
                buffer_frames: 9,
                ..request
            },
            &mixer,
            Some(&matrix)
        ),
        Err(CoreAudioError::Capacity)
    );
    assert_eq!(mixer.output_frame_basis(), basis);
}

#[test]
fn pure_coreaudio_cold_source_telemetry_allocation_refusal_keeps_original_owner_and_history() {
    let mut mixer = mixer();
    mixer.enable_source_playback_history(3).unwrap();
    mixer.render(&mut [0.; 4]).unwrap();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let history = mixer.source_playback_history().unwrap();
    let identity = (
        history.source_rate(),
        history.initial_physical_frame(),
        history.initial_playback_frame(),
        history.pulled_end_frame(),
        history.len(),
        history.capacity(),
    );
    assert!(matches!(
        crate::audio::fail_allocation(0, || prepare_render_telemetry(&mixer)),
        Err(CoreAudioError::Capacity)
    ));
    assert_eq!(mixer.output_frame_basis(), basis);
    assert_eq!(mixer.counters(), counters);
    assert_eq!(mixer.frame_cursor(), 4);
    let history = mixer.source_playback_history().unwrap();
    assert_eq!(
        (
            history.source_rate(),
            history.initial_physical_frame(),
            history.initial_playback_frame(),
            history.pulled_end_frame(),
            history.len(),
            history.capacity()
        ),
        identity
    );
    assert!(prepare_render_telemetry(&mixer).is_ok());
    mixer.render(&mut [0.; 4]).unwrap();
    assert_eq!(
        mixer.source_playback_history().unwrap().pulled_end_frame(),
        6
    );
}

#[test]
fn pure_coreaudio_default_cold_telemetry_requires_no_source_payload_allocation() {
    let mixer = mixer();
    assert!(crate::audio::fail_allocation(0, || prepare_render_telemetry(&mixer)).is_ok());
    assert!(mixer.source_playback_history().is_none());
}

#[test]
fn complete_public_open_preflight_returns_exact_original_owner_without_native_registration() {
    use crate::audio::{DeviceFormat, NativeOutputState, SampleEncoding};
    let valid = CoreAudioRequest {
        device: 1,
        format: AudioFormat::new(48_000, 2).unwrap(),
        buffer_frames: 4,
    };
    for request in [
        CoreAudioRequest { device: 0, ..valid },
        CoreAudioRequest {
            buffer_frames: 0,
            ..valid
        },
        CoreAudioRequest {
            format: AudioFormat::new(44_100, 2).unwrap(),
            ..valid
        },
        CoreAudioRequest {
            format: AudioFormat::new(48_000, 1).unwrap(),
            ..valid
        },
        CoreAudioRequest {
            buffer_frames: 9,
            ..valid
        },
    ] {
        let mut mixer = mixer();
        mixer.enable_source_playback_history(3).unwrap();
        let mut state = NativeOutputState::new(
            mixer,
            DeviceFormat::new(
                valid.format.sample_rate(),
                valid.format.channels(),
                SampleEncoding::Float32,
                None,
            )
            .unwrap(),
            None,
            8,
        )
        .unwrap_or_else(|_| panic!("valid prepared state"));
        let report = state.render_pending(4).unwrap();
        state.admit(1).unwrap();
        let pending = state.pending_samples().to_vec();
        let basis = state.output_frame_basis();
        let counters = state.mixer().counters();
        let clock = MachClock::new(ClockDomainId(2), ClockDomainId(3)).unwrap();
        let mut failure: CoreAudioStateOpenFailure =
            CoreAudioStream::open_state_recoverable(request, clock, state, None, 4)
                .err()
                .expect("invalid preflight must refuse before HAL lookup");
        assert_eq!(
            failure.error(),
            &if request.buffer_frames == 9 {
                CoreAudioError::Capacity
            } else {
                CoreAudioError::InvalidRequest
            }
        );
        assert!(failure.pending_owner().is_none());
        assert!(failure.cleanup_error().is_none());
        assert_eq!(
            CoreAudioStream::retry_state_open_cleanup(&mut failure),
            Ok(false)
        );
        let (_, recovered, pending_owner, cleanup) = failure.into_parts();
        assert!(pending_owner.is_none());
        assert!(cleanup.is_none());
        let recovered = recovered.expect("unique complete owner must be returned");
        assert_eq!(recovered.pending_report(), Some(report));
        assert_eq!(recovered.admitted_frames(), 1);
        assert_eq!(recovered.pending_samples(), pending);
        assert_eq!(recovered.output_frame_basis(), basis);
        assert_eq!(recovered.mixer().counters(), counters);
        let history = recovered.mixer().source_playback_history().unwrap();
        assert_eq!(
            (
                history.initial_physical_frame(),
                history.initial_playback_frame(),
                history.pulled_end_frame(),
                history.capacity()
            ),
            (2, 2, 6, 3)
        );
    }
}
