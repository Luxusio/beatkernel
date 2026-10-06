//! Deferred Windows-only pure preflight; no QPC/COM/events/device calls.
use super::*;
use beatkernel::audio::{AudioFormat, AudioLimits, MixerConfig, PcmLimits, SampleBank, command_queue};
use beatkernel::time::ClockDomainId;
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
fn request(backend: AudioBackendKind, mode: AudioStreamMode, rate: u32) -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("pure-preflight-only".into()),
        backend,
        mode,
        DeviceFormat::new(rate, 2, SampleEncoding::Float32, None).unwrap(),
        BufferRequest::DeviceDefault,
        PeriodRequest::DeviceDefault,
    )
    .unwrap()
}
#[test]
fn pure_wasapi_open_preflight_rejects_backend_format_and_invalid_timer_without_mutating_original_mixer()
 {
    let mixer = mixer();
    let basis = mixer.output_frame_basis();
    let counters = mixer.counters();
    let shared = AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault);
    assert_eq!(
        validate_open(
            &request(AudioBackendKind::Asio, shared, 48_000),
            &mixer,
            WasapiOptions::default()
        ),
        Err(AudioPlatformError::BackendUnavailable(
            AudioBackendKind::Asio
        ))
    );
    assert_eq!(
        validate_open(
            &request(AudioBackendKind::Wasapi, shared, 44_100),
            &mixer,
            WasapiOptions::default()
        ),
        Err(AudioPlatformError::InvalidFormat)
    );
    for interval in [
        0,
        -1,
        1,
        999_999,
        1_000_001,
        (i64::from(u32::MAX) + 1) * 1_000_000,
    ] {
        let options = WasapiOptions {
            mmcss_priority: None,
            wake_policy: WasapiWakePolicy::Timer {
                poll_interval: Duration::from_nanos(interval),
            },
        };
        assert_eq!(
            validate_open(
                &request(AudioBackendKind::Wasapi, shared, 48_000),
                &mixer,
                options
            ),
            Err(AudioPlatformError::InvalidRequest)
        );
    }
    let options = WasapiOptions {
        mmcss_priority: None,
        wake_policy: WasapiWakePolicy::Timer {
            poll_interval: Duration::from_nanos(1_000_000),
        },
    };
    assert!(
        validate_open(
            &request(AudioBackendKind::Wasapi, AudioStreamMode::Exclusive, 48_000),
            &mixer,
            options
        )
        .is_err()
    );
    assert_eq!(mixer.output_frame_basis(), basis);
    assert_eq!(mixer.counters(), counters);
    assert_eq!(mixer.frame_cursor(), 2);
}
#[test]
fn pure_wasapi_preflight_accepts_explicit_modes_and_whole_millisecond_boundaries_without_constructing_native_clock()
 {
    let mixer = mixer();
    let before = mixer.output_frame_basis();
    for mode in [
        AudioStreamMode::Exclusive,
        AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
        AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
    ] {
        assert_eq!(
            validate_open(
                &request(AudioBackendKind::Wasapi, mode, 48_000),
                &mixer,
                WasapiOptions::default()
            ),
            Ok(())
        );
    }
    for interval in [1_000_000, i64::from(u32::MAX) * 1_000_000] {
        let options = WasapiOptions {
            mmcss_priority: Some(WasapiPriority::Critical),
            wake_policy: WasapiWakePolicy::Timer {
                poll_interval: Duration::from_nanos(interval),
            },
        };
        assert_eq!(
            validate_open(
                &request(
                    AudioBackendKind::Wasapi,
                    AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
                    48_000
                ),
                &mixer,
                options
            ),
            Ok(())
        );
    }
    assert_eq!(mixer.output_frame_basis(), before);
}
