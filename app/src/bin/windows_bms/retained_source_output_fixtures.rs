//! Exercise the actual cold Windows selection/planner without opening a device.
//! Windows-only contracts below are Rust typing evidence, not native execution.
use super::{output_settings as settings, Backend};
use beatkernel::audio::{AudioFormat, AudioLimits, ChannelMatrix};
use beatkernel::time::Duration;
use beatkernel_bms_runtime::{
    gameplay::output::domain::control::OutputCapability, settings::SettingsHost,
};
use beatkernel_platform::audio::{
    validate_known_request_sizing, AppliedStreamConfig, AudioBackendKind, AudioDeviceId,
    AudioStreamMode, AudioStreamRequest, BufferRequest, DeviceFormat, NegotiationPolicy,
    PeriodRequest, SampleEncoding, SharedPeriodPolicy,
};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn source() -> AudioFormat {
    AudioFormat::new(48_000, 2).unwrap()
}
fn current() -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("retained-endpoint-A".into()),
        AudioBackendKind::Wasapi,
        AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
        DeviceFormat::new(48_000, 2, SampleEncoding::Float32, Some(3)).unwrap(),
        BufferRequest::Frames(64),
        PeriodRequest::Frames(32),
    )
    .unwrap()
    .with_negotiation(NegotiationPolicy::AllowSupportedRounding)
}
fn cap() -> OutputCapability {
    OutputCapability {
        host: SettingsHost::Windows,
        current_args: strings(&["--backend", "wasapi", "--device", "retained-endpoint-A"]),
    }
}
fn applied(requested: AudioStreamRequest) -> AppliedStreamConfig {
    AppliedStreamConfig {
        format: requested.format(),
        requested,
        buffer_frames: 96,
        buffer_duration: Duration::from_nanos(2_000_000),
        period_frames: Some(48),
        period_duration: Duration::from_nanos(1_000_000),
        stream_latency: Duration::from_nanos(3_000_000),
        sizing_adjusted: true,
    }
}

fn planned_sizing_request(policy: NegotiationPolicy, args: &[String]) -> AudioStreamRequest {
    let original = current().with_negotiation(policy);
    let resolved = super::plan_retained_shared_output(&cap(), &original, source(), None, args)
        .unwrap()
        .into_request(None)
        .unwrap();
    assert_eq!(resolved.native.device(), original.device());
    assert_eq!(resolved.native.format(), original.format());
    assert_eq!(resolved.native.negotiation(), policy);
    assert!(resolved.matrix.is_none());
    resolved.native
}

#[test]
fn retained_source_complete_startup_selects_only_retained_shared_wasapi() {
    for retained in [false, true] {
        for backend in [Backend::Wasapi, Backend::Asio] {
            for mode in [
                AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
                AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
                AudioStreamMode::Exclusive,
            ] {
                assert_eq!(
                    super::retained_complete_startup(retained, backend, mode),
                    retained && backend == Backend::Wasapi && mode != AudioStreamMode::Exclusive,
                    "retained={retained} backend={backend:?} mode={mode:?}",
                );
            }
        }
    }
}

#[test]
fn retained_source_unchanged_draft_keeps_original_request_and_source_interpretation() {
    for encoding in [
        SampleEncoding::Float32,
        SampleEncoding::Pcm {
            container_bits: 16,
            valid_bits: 16,
        },
        SampleEncoding::Pcm {
            container_bits: 32,
            valid_bits: 24,
        },
    ] {
        let original = AudioStreamRequest::new(
            current().device().clone(),
            AudioBackendKind::Wasapi,
            current().mode(),
            DeviceFormat::new(48_000, 2, encoding, Some(3)).unwrap(),
            current().buffer(),
            current().period(),
        )
        .unwrap()
        .with_negotiation(current().negotiation());
        let plan = super::plan_retained_shared_output(&cap(), &original, source(), None, &[])
            .unwrap()
            .into_request(None)
            .unwrap();
        assert_eq!(plan.native, original);
        assert!(plan.matrix.is_none());
    }
}

#[test]
fn retained_source_shared_geometry_and_endpoint_changes_keep_original_pcm_metadata() {
    let original = current();
    for (args, mode, buffer, period) in [
        (
            strings(&[
                "--device",
                "retained-endpoint-B",
                "--buffer",
                "frames:128",
                "--period",
                "frames:16",
            ]),
            AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
            BufferRequest::Frames(128),
            PeriodRequest::Frames(16),
        ),
        (
            strings(&[
                "--device",
                "retained-endpoint-B",
                "--shared-policy",
                "legacy",
                "--buffer",
                "ns:4000000",
                "--period",
                "default",
            ]),
            AudioStreamMode::Shared(SharedPeriodPolicy::DeviceDefault),
            BufferRequest::Duration(Duration::from_nanos(4_000_000)),
            PeriodRequest::DeviceDefault,
        ),
        (
            strings(&[
                "--device",
                "retained-endpoint-B",
                "--buffer",
                "default",
                "--period",
                "ns:500000",
            ]),
            AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod),
            BufferRequest::DeviceDefault,
            PeriodRequest::Duration(Duration::from_nanos(500_000)),
        ),
    ] {
        let resolved = super::plan_retained_shared_output(&cap(), &original, source(), None, &args)
            .unwrap()
            .into_request(None)
            .unwrap();
        assert_eq!(resolved.native.backend(), AudioBackendKind::Wasapi);
        assert_eq!(resolved.native.device().0, "retained-endpoint-B");
        assert_eq!(resolved.native.mode(), mode);
        assert_eq!(resolved.native.buffer(), buffer);
        assert_eq!(resolved.native.period(), period);
        assert_eq!(resolved.native.format(), original.format());
        assert_eq!(resolved.native.negotiation(), original.negotiation());
        assert!(resolved.matrix.is_none());
    }
}

#[test]
fn retained_source_remix_always_uses_original_source_columns_across_repeated_changes() {
    let original = current();
    let mono = ChannelMatrix::new(2, 1, &[0.5, 0.25]).unwrap();
    let first = super::plan_retained_shared_output(
        &cap(),
        &original,
        source(),
        None,
        &strings(&["--output-matrix", "0.5,0.25"]),
    )
    .unwrap()
    .into_request(None)
    .unwrap();
    assert_eq!(first.matrix, Some(mono.clone()));
    assert_eq!(first.native.format().channels(), 1);
    assert_eq!(first.native.format().channel_mask(), None);
    assert_eq!(first.native.format().sample_rate(), source().sample_rate());
    assert_eq!(
        first.native.format().encoding(),
        original.format().encoding()
    );

    let same = super::plan_retained_shared_output(
        &cap(),
        &first.native,
        source(),
        first.matrix.as_ref(),
        &strings(&["--buffer", "frames:256"]),
    )
    .unwrap()
    .into_request(None)
    .unwrap();
    assert_eq!(same.matrix, Some(mono));
    assert_eq!(same.native.format(), first.native.format());
    assert_eq!(same.native.buffer(), BufferRequest::Frames(256));

    let changed = super::plan_retained_shared_output(
        &cap(),
        &same.native,
        source(),
        same.matrix.as_ref(),
        &strings(&["--output-matrix", "1,0;0,1;0.5,0.5"]),
    )
    .unwrap()
    .into_request(None)
    .unwrap();
    assert_eq!(changed.native.format().channels(), 3);
    assert_eq!(
        changed.matrix,
        Some(ChannelMatrix::new(2, 3, &[1.0, 0.0, 0.0, 1.0, 0.5, 0.5]).unwrap())
    );
    let exact = super::plan_retained_shared_output(
        &cap(),
        &changed.native,
        source(),
        changed.matrix.as_ref(),
        &strings(&["--output-matrix", "exact"]),
    )
    .unwrap()
    .into_request(None)
    .unwrap();
    assert!(exact.matrix.is_none());
    assert_eq!(exact.native.format().channels(), source().channels());
    assert_eq!(exact.native.format().sample_rate(), source().sample_rate());
    assert_eq!(
        exact.native.format().encoding(),
        original.format().encoding()
    );
}

#[test]
fn retained_source_same_width_remix_preserves_native_channel_mask() {
    let original = current();
    let remixed = super::plan_retained_shared_output(
        &cap(),
        &original,
        source(),
        None,
        &strings(&["--output-matrix", "0,1;1,0"]),
    )
    .unwrap()
    .into_request(None)
    .unwrap();
    assert_eq!(remixed.native.format(), original.format());
    assert_eq!(
        remixed.matrix,
        Some(ChannelMatrix::new(2, 2, &[0.0, 1.0, 1.0, 0.0]).unwrap())
    );
}

#[test]
fn retained_source_unsupported_mode_backend_rate_and_encoding_refuse_cold() {
    let original = current();
    let capability = cap();
    let previous_args = capability.current_args.clone();
    for args in [
        strings(&["--mode", "exclusive"]),
        strings(&["--backend", "asio"]),
        strings(&["--rate", "44100"]),
        strings(&["--encoding", "pcm16"]),
        strings(&["--channels", "1"]),
    ] {
        assert!(
            super::plan_retained_shared_output(&capability, &original, source(), None, &args)
                .is_err(),
            "unsupported draft {args:?} must not silently select another path",
        );
        assert_eq!(capability.current_args, previous_args);
        assert_eq!(original, current());
    }
    assert!(super::plan_retained_shared_output(
        &capability,
        &original,
        AudioFormat::new(44_100, 2).unwrap(),
        None,
        &[],
    )
    .is_err());
    for (backend, mode) in [
        (AudioBackendKind::Wasapi, AudioStreamMode::Exclusive),
        (AudioBackendKind::Asio, original.mode()),
    ] {
        let unsupported = AudioStreamRequest::new(
            original.device().clone(),
            backend,
            mode,
            original.format(),
            original.buffer(),
            original.period(),
        )
        .unwrap();
        assert!(super::plan_retained_shared_output(
            &capability,
            &unsupported,
            source(),
            None,
            &strings(&["--backend", "wasapi", "--mode", "shared"]),
        )
        .is_err());
    }
}

#[test]
fn retained_source_invalid_geometry_matrix_and_endpoint_never_produce_a_native_request() {
    let original = current();
    let too_many = format!("frames:{}", AudioLimits::MAX_RENDER_FRAMES + 1);
    for args in [
        strings(&["--buffer", "frames:0"]),
        strings(&["--period", "ns:0"]),
        strings(&["--buffer", &too_many]),
        strings(&["--period", &too_many]),
        strings(&["--shared-policy", "legacy"]),
        strings(&["--output-matrix", "1"]),
        strings(&["--output-matrix", "1,0;1"]),
        strings(&["--output-matrix", "NaN,1"]),
        strings(&["--device", "bad\0endpoint"]),
        strings(&["--device", "{12345678-9ABC-DEF0-1234-56789ABCDEF0}"]),
    ] {
        assert!(
            super::plan_retained_shared_output(&cap(), &original, source(), None, &args).is_err(),
            "invalid draft {args:?}",
        );
    }
    let wrong_source = ChannelMatrix::new(1, 1, &[1.0]).unwrap();
    assert!(super::plan_retained_shared_output(
        &cap(),
        &original,
        source(),
        Some(&wrong_source),
        &[],
    )
    .is_err());
}

#[test]
fn retained_source_duration_capacity_refuses_after_actual_planning_and_request_resolution() {
    // The first frame beyond the actual render allocation ceiling, at 48 kHz.
    let oversized_nanos =
        ((AudioLimits::MAX_RENDER_FRAMES as u128 + 1) * 1_000_000_000).div_ceil(48_000);
    let duration = format!("ns:{oversized_nanos}");
    for policy in [
        NegotiationPolicy::Exact,
        NegotiationPolicy::AllowSupportedRounding,
    ] {
        for flag in ["--buffer", "--period"] {
            let requested = planned_sizing_request(policy, &strings(&[flag, &duration]));
            let before = requested.clone();
            assert!(
                validate_known_request_sizing(&requested, AudioLimits::MAX_RENDER_FRAMES).is_err(),
                "{flag} beyond allocated render capacity cannot be admitted with {policy:?}",
            );
            assert_eq!(requested, before);
        }
    }
}

#[test]
fn retained_source_fractional_duration_requires_explicit_rounding_after_actual_planning() {
    // 1 ms is exactly 48 frames; one more nanosecond is not a complete frame.
    for flag in ["--buffer", "--period"] {
        let args = strings(&[flag, "ns:1000001"]);
        let exact = planned_sizing_request(NegotiationPolicy::Exact, &args);
        let rounded = planned_sizing_request(NegotiationPolicy::AllowSupportedRounding, &args);
        assert!(validate_known_request_sizing(&exact, AudioLimits::MAX_RENDER_FRAMES).is_err());
        assert!(validate_known_request_sizing(&rounded, AudioLimits::MAX_RENDER_FRAMES).is_ok());
        // Cold validation preserves the caller's duration and explicit policy.
        assert_eq!(exact.buffer(), rounded.buffer());
        assert_eq!(exact.period(), rounded.period());
        assert_eq!(
            if flag == "--buffer" {
                match rounded.buffer() {
                    BufferRequest::Duration(duration) => duration,
                    _ => panic!("the requested buffer remains a duration"),
                }
            } else {
                match rounded.period() {
                    PeriodRequest::Duration(duration) => duration,
                    _ => panic!("the requested period remains a duration"),
                }
            },
            Duration::from_nanos(1_000_001),
        );
    }
}

#[test]
fn retained_source_representable_and_unknown_default_sizes_remain_admissible() {
    let maximum = format!("frames:{}", AudioLimits::MAX_RENDER_FRAMES);
    // Three frames have an exact integer-nanosecond duration at 48 kHz.
    let whole_frames = AudioLimits::MAX_RENDER_FRAMES / 3 * 3;
    let near_limit_duration = format!("ns:{}", whole_frames as u128 * 62_500 / 3);
    for policy in [
        NegotiationPolicy::Exact,
        NegotiationPolicy::AllowSupportedRounding,
    ] {
        for args in [
            strings(&["--buffer", &maximum, "--period", &maximum]),
            strings(&[
                "--buffer",
                &near_limit_duration,
                "--period",
                &near_limit_duration,
            ]),
            strings(&["--buffer", "ns:1000000", "--period", "ns:1000000"]),
            strings(&["--buffer", "default", "--period", "default"]),
            strings(&[
                "--shared-policy",
                "legacy",
                "--buffer",
                "default",
                "--period",
                "default",
            ]),
        ] {
            let requested = planned_sizing_request(policy, &args);
            let before = requested.clone();
            validate_known_request_sizing(&requested, AudioLimits::MAX_RENDER_FRAMES)
                .unwrap_or_else(|error| panic!("valid cold draft {args:?}: {error}"));
            assert_eq!(requested, before);
        }
    }
}

#[test]
fn retained_source_capability_roundtrip_uses_new_owned_config_and_original_source() {
    let original = current();
    let request = super::plan_retained_shared_output(
        &cap(),
        &original,
        source(),
        None,
        &strings(&[
            "--device",
            "applied-endpoint-B",
            "--buffer",
            "frames:128",
            "--period",
            "frames:16",
            "--output-matrix",
            "0.5,0.25",
        ]),
    )
    .unwrap()
    .into_request(None)
    .unwrap();
    let configuration = applied(request.native.clone());
    let current_capability = settings::switch_capability(
        settings::capability(&configuration, request.matrix.as_ref()).unwrap(),
        Backend::Wasapi,
        None,
    )
    .unwrap();
    let fields = current_capability.settings().unwrap();
    for (flag, expected) in [
        ("--backend", "wasapi"),
        ("--device", "applied-endpoint-B"),
        ("--mode", "shared"),
        ("--shared-policy", "engine"),
        ("--buffer", "frames:128"),
        ("--period", "frames:16"),
        ("--output-matrix", "0.5,0.25"),
    ] {
        assert_eq!(
            fields
                .fields()
                .iter()
                .find(|field| field.flag == flag)
                .unwrap()
                .value,
            expected,
            "capability {flag}",
        );
    }
    let roundtrip = super::plan_retained_shared_output(
        &current_capability,
        &configuration.requested,
        source(),
        request.matrix.as_ref(),
        &current_capability.current_args,
    )
    .unwrap()
    .into_request(None)
    .unwrap();
    assert_eq!(roundtrip.native, request.native);
    assert_eq!(roundtrip.matrix, request.matrix);
    assert_eq!(
        roundtrip.matrix.unwrap().source_channels(),
        source().channels()
    );
}

#[test]
fn retained_source_inconsistent_applied_capability_refuses_before_publication() {
    let valid = applied(current());
    for invalid in [
        AppliedStreamConfig {
            buffer_frames: 0,
            ..valid.clone()
        },
        AppliedStreamConfig {
            buffer_frames: AudioLimits::MAX_RENDER_FRAMES as u32 + 1,
            ..valid.clone()
        },
        AppliedStreamConfig {
            format: DeviceFormat::new(44_100, 2, SampleEncoding::Float32, Some(3)).unwrap(),
            ..valid.clone()
        },
        AppliedStreamConfig {
            format: DeviceFormat::new(
                48_000,
                2,
                SampleEncoding::Pcm {
                    container_bits: 16,
                    valid_bits: 16,
                },
                Some(3),
            )
            .unwrap(),
            ..valid.clone()
        },
    ] {
        assert!(settings::capability(&invalid, None).is_err());
    }
    assert!(settings::capability(
        &valid,
        Some(&ChannelMatrix::new(2, 1, &[0.5, 0.5]).unwrap()),
    )
    .is_err());
}

#[cfg(target_os = "windows")]
#[test]
fn retained_source_windows_owner_exposes_complete_state_and_mutable_conditional_ports() {
    use super::owned_output::{self, WindowsOutputOwner};
    use beatkernel::audio::{InterpolatedTargetTime, Mixer};
    use beatkernel::time::ClockPoint;
    use beatkernel_bms_runtime::{
        audio_authority::AudioAuthorityCoordinateBasis,
        gameplay::output::ports::SourcePlaybackIdentity,
        gameplay_presentation::GameplayAudioOutputContext,
        native_audio_presentation::NativeAudioPresentation,
        native_coordinate_control::CoordinateControlFacts,
        native_playback_phase::NativePlaybackMapping,
    };
    use beatkernel_platform::windows::clock::QpcClock;

    let _: fn(AudioStreamRequest, Mixer, QpcClock) -> super::Result<WindowsOutputOwner> =
        owned_output::complete_owner;
    let _: fn(&mut WindowsOutputOwner) -> super::Result<Option<AudioAuthorityCoordinateBasis>> =
        WindowsOutputOwner::source_coordinate_basis;
    let _: fn(
        &mut WindowsOutputOwner,
        &mut NativeAudioPresentation,
        SourcePlaybackIdentity,
    ) -> super::Result<Option<CoordinateControlFacts>> = WindowsOutputOwner::observe_native_control;
    let _: fn(
        &mut WindowsOutputOwner,
        &NativeAudioPresentation,
        &InterpolatedTargetTime,
        SourcePlaybackIdentity,
    ) -> super::Result<Option<NativePlaybackMapping>> =
        WindowsOutputOwner::conditional_playback_mapping_at;
    let _: fn(
        &mut WindowsOutputOwner,
        &NativeAudioPresentation,
        &InterpolatedTargetTime,
        SourcePlaybackIdentity,
        Option<u64>,
    ) -> super::Result<Option<u64>> =
        WindowsOutputOwner::request_source_history_retirement_with_source_pin;
    let _: fn(
        &mut owned_output::WindowsOutputUi,
        &mut WindowsOutputOwner,
        GameplayAudioOutputContext<'_>,
        ClockPoint,
        SourcePlaybackIdentity,
        bool,
    ) -> super::Result<bool> = owned_output::WindowsOutputUi::service_coordinate_source_audio;
}
