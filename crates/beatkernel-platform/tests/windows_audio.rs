#![cfg(target_os = "windows")]

use beatkernel::{
    audio::*,
    time::{ClockDomainId, Duration, Timestamp},
};
use beatkernel_platform::{
    audio::*,
    windows::{
        audio::{WasapiBackend, WasapiOptions, WasapiPriority, WasapiWakePolicy},
        clock::QpcClock,
    },
};
use std::{
    collections::HashSet,
    thread,
    time::{Duration as WallDuration, Instant},
};

fn format() -> DeviceFormat {
    DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(3)).unwrap()
}
fn request(backend: AudioBackendKind, mode: AudioStreamMode) -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("beatkernel-test-deliberately-absent-endpoint".into()),
        backend,
        mode,
        format(),
        BufferRequest::DeviceDefault,
        PeriodRequest::DeviceDefault,
    )
    .unwrap()
}
fn mixer(format: DeviceFormat) -> (CommandProducer, Mixer) {
    let pcm = format.pcm();
    let pcm_limits = PcmLimits::new(4_194_304, 4_194_304, 1).unwrap();
    let mut bank = SampleBank::new(pcm, pcm_limits).unwrap();
    let samples = vec![0.1; pcm.sample_rate() as usize / 4 * usize::from(pcm.channels())];
    bank.insert(
        SampleId(1),
        PcmSample::new(pcm, samples, pcm_limits).unwrap(),
    )
    .unwrap();
    let limits = AudioLimits::new(4, 2, 4, AudioLimits::MAX_RENDER_FRAMES, 4).unwrap();
    let config = MixerConfig::new(pcm, ClockDomainId(93), Timestamp::ZERO, limits);
    let (producer, consumer) = command_queue(4).unwrap();
    (producer, Mixer::new(config, bank, consumer).unwrap())
}

#[test]
fn default_options_are_explicit_normal_priority_and_event_wakes() {
    let options = WasapiOptions::default();
    assert_eq!(options.mmcss_priority, Some(WasapiPriority::Normal));
    assert_eq!(options.wake_policy, WasapiWakePolicy::EventDriven);
}

#[test]
fn enumeration_preserves_unique_native_identities_and_absent_device_probes_fail() {
    let backend = WasapiBackend;
    let mode = AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault);
    for identity in ["", "valid-prefix\0other-endpoint"] {
        let malformed = AudioDeviceId(identity.into());
        assert_eq!(
            backend.mix_format(&malformed),
            Err(AudioPlatformError::InvalidRequest)
        );
        assert_eq!(
            backend.supports_format(&malformed, mode, format()),
            Err(AudioPlatformError::InvalidRequest)
        );
        assert_eq!(
            backend.period_constraints(&malformed, mode, format()),
            Err(AudioPlatformError::InvalidRequest)
        );
    }
    let devices = backend.devices().unwrap();
    let mut identities = HashSet::new();
    for device in devices {
        assert!(!device.id.0.is_empty());
        assert!(identities.insert(device.id));
    }
    // Zero endpoints is valid enumeration evidence, never playback evidence.
    let absent = request(
        AudioBackendKind::Wasapi,
        AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
    );
    assert_eq!(
        backend.mix_format(absent.device()),
        Err(AudioPlatformError::DeviceUnavailable)
    );
    assert_eq!(
        backend.supports_format(absent.device(), absent.mode(), absent.format()),
        Err(AudioPlatformError::DeviceUnavailable)
    );
    assert_eq!(
        backend.period_constraints(absent.device(), absent.mode(), absent.format()),
        Err(AudioPlatformError::DeviceUnavailable)
    );
}

#[test]
fn repeated_absent_endpoint_open_failure_and_asio_remain_specific() {
    let backend = WasapiBackend;
    let clock = QpcClock::new(ClockDomainId(93)).unwrap();
    for _ in 0..8 {
        let request = request(
            AudioBackendKind::Wasapi,
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
        );
        let (_producer, mixer) = mixer(request.format());
        assert!(matches!(
            backend.open(request, mixer, clock, WasapiOptions::default()),
            Err(AudioPlatformError::DeviceUnavailable)
        ));
    }
    let request = request(AudioBackendKind::Asio, AudioStreamMode::Exclusive);
    let (_producer, mixer) = mixer(request.format());
    assert!(matches!(
        backend.open(request, mixer, clock, WasapiOptions::default()),
        Err(AudioPlatformError::AsioLicenseUnresolved)
    ));
}

#[test]
fn timer_wakes_require_positive_whole_milliseconds_and_explicit_shared_mode() {
    let backend = WasapiBackend;
    let clock = QpcClock::new(ClockDomainId(93)).unwrap();
    for ns in [0, -1, 1, 999_999, 1_000_001] {
        let request = request(
            AudioBackendKind::Wasapi,
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
        );
        let (_producer, mixer) = mixer(request.format());
        let options = WasapiOptions {
            mmcss_priority: None,
            wake_policy: WasapiWakePolicy::Timer {
                poll_interval: Duration::from_nanos(ns),
            },
        };
        assert!(matches!(
            backend.open(request, mixer, clock, options),
            Err(AudioPlatformError::InvalidRequest)
        ));
    }
    let request = request(AudioBackendKind::Wasapi, AudioStreamMode::Exclusive);
    let (_producer, mixer) = mixer(request.format());
    let options = WasapiOptions {
        mmcss_priority: None,
        wake_policy: WasapiWakePolicy::Timer {
            poll_interval: Duration::from_nanos(1_000_000),
        },
    };
    assert!(matches!(
        backend.open(request, mixer, clock, options),
        Err(AudioPlatformError::ConfigurationUnsupported {
            constraint: ConfigurationConstraint::TimerRequiresShared,
            ..
        })
    ));
}

fn coherent_snapshot(stream: &impl AudioOutputStream) -> AudioStreamSnapshot {
    let deadline = Instant::now() + WallDuration::from_secs(2);
    loop {
        let snapshot = stream.snapshot();
        if snapshot.telemetry_available {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "native telemetry did not become coherent"
        );
        thread::yield_now();
    }
}

fn native_smoke(mode: AudioStreamMode) {
    let device = AudioDeviceId(
        std::env::var("BEATKERNEL_AUDIO_DEVICE")
            .expect("set BEATKERNEL_AUDIO_DEVICE to an explicit active endpoint"),
    );
    let backend = WasapiBackend;
    let format = backend.mix_format(&device).unwrap();
    assert_eq!(
        backend.supports_format(&device, mode, format).unwrap(),
        FormatSupport::Exact,
        "selected endpoint/configuration must support the explicitly requested mode"
    );
    let request = AudioStreamRequest::new(
        device,
        AudioBackendKind::Wasapi,
        mode,
        format,
        BufferRequest::DeviceDefault,
        PeriodRequest::DeviceDefault,
    )
    .unwrap();
    let clock = QpcClock::new(ClockDomainId(93)).unwrap();
    for _ in 0..2 {
        let (mut producer, mixer) = mixer(format);
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 0.2,
            })
            .unwrap();
        let mut stream = backend
            .open(request.clone(), mixer, clock, WasapiOptions::default())
            .unwrap();
        assert_eq!(stream.configuration().requested, request);
        assert_eq!(stream.configuration().format, format);
        assert!(stream.configuration().buffer_frames > 0);
        assert_eq!(stream.options().wake_policy, WasapiWakePolicy::EventDriven);
        let primed = coherent_snapshot(&stream);
        assert_eq!(primed.status, AudioStreamStatus::Ready);
        stream.start().unwrap();
        assert_eq!(stream.snapshot().status, AudioStreamStatus::Running);
        let deadline = Instant::now() + WallDuration::from_secs(2);
        let first = loop {
            let snapshot = coherent_snapshot(&stream);
            assert_eq!(snapshot.status, AudioStreamStatus::Running);
            if let Some(clock) = snapshot.clock {
                break clock;
            }
            assert!(Instant::now() < deadline, "no native clock after Start");
            thread::sleep(WallDuration::from_millis(5));
        };
        let last = loop {
            thread::sleep(WallDuration::from_millis(10));
            let snapshot = coherent_snapshot(&stream);
            assert_eq!(snapshot.status, AudioStreamStatus::Running);
            if snapshot
                .clock
                .is_some_and(|clock| clock.position > first.position)
                && snapshot.counters.submitted_frames > primed.counters.submitted_frames
                && snapshot.counters.buffer_fills > primed.counters.buffer_fills
            {
                break snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "native submitted frames/clock failed to progress"
            );
        };
        let latest_clock = last.clock.unwrap();
        assert!(latest_clock.frequency > 0);
        assert_eq!(latest_clock.frequency, first.frequency);
        assert!(latest_clock.qpc_100ns >= first.qpc_100ns);
        assert_eq!(
            latest_clock.host_point.unwrap().domain,
            clock.output_domain()
        );
        assert!(last.render.unwrap().frames > 0);
        assert_eq!(last.counters.native_failures, 0);
        stream.stop().unwrap();
        assert_eq!(stream.snapshot().status, AudioStreamStatus::Stopped);
        assert!(stream.start().is_err(), "stopped stream must be reopened");
        drop(stream);
        drop(producer);
    }
}

#[test]
#[ignore = "requires explicit endpoint and BEATKERNEL_AUDIO_UNSUPPORTED_RATE known to fail native probing"]
fn unsupported_format_suggestion_is_advisory_and_never_opened_automatically() {
    let device = AudioDeviceId(
        std::env::var("BEATKERNEL_AUDIO_DEVICE").expect("select an explicit endpoint"),
    );
    let rate: u32 = std::env::var("BEATKERNEL_AUDIO_UNSUPPORTED_RATE")
        .expect("select a valid sample rate known to be unsupported")
        .parse()
        .unwrap();
    let backend = WasapiBackend;
    let native_mix = backend.mix_format(&device).unwrap();
    let requested = DeviceFormat::new(
        rate,
        native_mix.channels(),
        native_mix.encoding(),
        native_mix.channel_mask(),
    )
    .unwrap();
    let mode = AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault);
    let FormatSupport::Unsupported { closest } =
        backend.supports_format(&device, mode, requested).unwrap()
    else {
        panic!("selected rate is supported; this fixture requires a native unsupported-format response")
    };
    let request = AudioStreamRequest::new(
        device,
        AudioBackendKind::Wasapi,
        mode,
        requested,
        BufferRequest::DeviceDefault,
        PeriodRequest::DeviceDefault,
    )
    .unwrap();
    let (_producer, mixer) = mixer(requested);
    let clock = QpcClock::new(ClockDomainId(93)).unwrap();
    assert!(
        matches!(backend.open(request, mixer, clock, WasapiOptions::default()),
        Err(AudioPlatformError::FormatUnsupported { closest: returned }) if returned == closest)
    );
}

#[test]
#[ignore = "requires explicit BEATKERNEL_AUDIO_DEVICE and a native shared endpoint; compilation is not playback evidence"]
fn selected_endpoint_shared_submits_pcm_progresses_clock_stops_and_reopens() {
    native_smoke(AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod));
}
#[test]
#[ignore = "requires explicit BEATKERNEL_AUDIO_DEVICE with supported exclusive mix format; missing endpoint is a failure"]
fn selected_endpoint_exclusive_submits_pcm_progresses_clock_stops_and_reopens() {
    native_smoke(AudioStreamMode::Exclusive);
}
