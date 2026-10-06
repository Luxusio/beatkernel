//! Deferred macOS-only pure preflight; no MachClock/native acquisition.
use super::*;
use beatkernel::audio::{AudioLimits, MixerConfig, PcmLimits, SampleBank, command_queue};
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
fn pure_coreaudio_preflight_rejects_invalid_device_buffer_format_and_capacity_before_native_ownership()
 {
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
fn pure_coreaudio_exact_capacity_boundary_accepts_without_resolving_device_or_constructing_mach_clock()
 {
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
