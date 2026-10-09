//! Admission tests call the production planner without opening a native device.
use super::{output_settings as settings, AsioView};
use beatkernel::audio::{AudioFormat, AudioLimits, ChannelMatrix};
use beatkernel_bms_runtime::{
    gameplay::output::domain::control::OutputCapability, settings::SettingsHost,
};
use beatkernel_platform::audio::{
    asio::AsioBufferRequest, AudioBackendKind, AudioDeviceId, AudioStreamMode, AudioStreamRequest,
    BufferRequest, DeviceFormat, NegotiationPolicy, PeriodRequest, SampleEncoding,
    SharedPeriodPolicy,
};

const DRIVER: &str = "{12345678-9ABC-DEF0-1234-56789ABCDEF0}";

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn set(args: &mut [String], flag: &str, value: &str) {
    let index = args.iter().position(|arg| arg == flag).unwrap();
    args[index + 1] = value.into();
}
fn source() -> AudioFormat {
    AudioFormat::new(48_000, 2).unwrap()
}
fn wasapi() -> AudioStreamRequest {
    AudioStreamRequest::new(
        AudioDeviceId("endpoint-A".into()),
        AudioBackendKind::Wasapi,
        AudioStreamMode::Exclusive,
        DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap(),
        BufferRequest::Frames(64),
        PeriodRequest::DeviceDefault,
    )
    .unwrap()
    .with_negotiation(NegotiationPolicy::AllowSupportedRounding)
}
fn wasapi_cap() -> OutputCapability {
    OutputCapability {
        host: SettingsHost::Windows,
        current_args: strings(&["--backend", "wasapi", "--device", "endpoint-A"]),
    }
}
fn asio_args() -> Vec<String> {
    strings(&[
        "--backend",
        "asio",
        "--device",
        DRIVER,
        "--asio-view",
        "native",
        "--output-channels",
        "3,1",
        "--buffer",
        "frames:257",
        "--asio-system-clock",
        "multimedia",
        "--asio-timer-error-ns",
        "10",
        "--asio-drift-error-ns",
        "20",
        "--asio-latency-error-ns",
        "30",
        "--asio-anchor-age-ns",
        "1000000000",
        "--output-matrix",
        "exact",
    ])
}
fn asio_cap() -> OutputCapability {
    OutputCapability {
        host: SettingsHost::Windows,
        current_args: asio_args(),
    }
}
fn from_wasapi(args: &[String], available: bool) -> Result<settings::TargetPlan, String> {
    settings::plan_target(
        &wasapi_cap(),
        Some(&wasapi()),
        source(),
        None,
        args,
        available,
    )
}

#[test]
fn legacy_requests_keep_the_current_backend_and_native_policy() {
    let mut cap = wasapi_cap();
    cap.current_args.drain(..2);
    let settings::TargetPlan::Wasapi(plan) =
        settings::plan_target(&cap, Some(&wasapi()), source(), None, &[], false).unwrap()
    else {
        panic!("legacy WASAPI request changed backend")
    };
    assert!(plan.matrix.is_none());
    assert_eq!(plan.into_request(None).unwrap().native, wasapi());

    let mut cap = asio_cap();
    cap.current_args.drain(..2);
    let settings::TargetPlan::Asio(plan) =
        settings::plan_target(&cap, None, source(), None, &[], true).unwrap()
    else {
        panic!("legacy ASIO channel declaration lost backend")
    };
    assert_eq!(plan.device, DRIVER);
    assert_eq!(plan.channels, [3, 1]);
    assert_eq!(plan.sample_rate, 48_000);
}

#[test]
fn selecting_asio_preserves_source_rate_and_explicit_driver_clock_routing() {
    let settings::TargetPlan::Asio(plan) = from_wasapi(&asio_args(), true).unwrap() else {
        panic!("selected ASIO did not route to ASIO")
    };
    assert_eq!(plan.device, DRIVER);
    assert_eq!(plan.view, AsioView::Native);
    assert_eq!(plan.channels, [3, 1]);
    assert_eq!(plan.buffer, AsioBufferRequest::Frames(257));
    assert_eq!(plan.sample_rate, 48_000);
    assert_eq!(
        plan.bounds,
        settings::AsioClockBounds {
            timer: 10,
            drift: 20,
            latency: 30,
            age: 1_000_000_000
        }
    );
    assert!(plan.matrix.is_none());
    assert!(
        from_wasapi(&asio_args(), false).is_err(),
        "SDK-disabled admission must refuse before opening"
    );
}

#[test]
fn first_asio_selection_requires_every_explicit_metadata_field() {
    for flag in [
        "--device",
        "--asio-view",
        "--output-channels",
        "--asio-system-clock",
        "--asio-timer-error-ns",
        "--asio-drift-error-ns",
        "--asio-latency-error-ns",
        "--asio-anchor-age-ns",
    ] {
        let mut args = asio_args();
        let index = args.iter().position(|arg| arg == flag).unwrap();
        args.drain(index..index + 2);
        assert!(from_wasapi(&args, true).is_err(), "missing {flag}");
        let mut args = asio_args();
        set(&mut args, flag, "");
        assert!(from_wasapi(&args, true).is_err(), "blank {flag}");
    }
}

#[test]
fn malformed_target_metadata_is_rejected_by_the_real_planner() {
    for (flag, value) in [
        ("--backend", "auto"),
        ("--device", "endpoint-A"),
        ("--device", "{00000000-0000-0000-0000-000000000000}"),
        ("--asio-view", "x64"),
        ("--output-channels", "0,0"),
        ("--output-channels", "0,-1"),
        ("--output-channels", "2147483648"),
        ("--asio-system-clock", "qpc"),
        ("--asio-timer-error-ns", "-1"),
        ("--asio-drift-error-ns", "NaN"),
        ("--asio-latency-error-ns", "9223372036854775808"),
        ("--asio-anchor-age-ns", "0"),
        ("--asio-anchor-age-ns", "2147483648000000"),
    ] {
        let mut args = asio_args();
        set(&mut args, flag, value);
        assert!(from_wasapi(&args, true).is_err(), "{flag} {value}");
    }
}

#[test]
fn asio_target_ignores_inactive_wasapi_fields_but_enforces_its_shared_buffer() {
    let mut args = asio_args();
    args.extend(strings(&[
        "--mode",
        "invalid-inactive",
        "--period",
        "not-a-period",
        "--shared-policy",
        "not-a-policy",
    ]));
    assert!(matches!(
        from_wasapi(&args, true).unwrap(),
        settings::TargetPlan::Asio(_)
    ));
    for buffer in ["ns:1000000", "frames:0", "frames:4294967296"] {
        set(&mut args, "--buffer", buffer);
        assert!(
            from_wasapi(&args, true).is_err(),
            "target ASIO buffer {buffer}"
        );
    }
    set(
        &mut args,
        "--buffer",
        &format!("frames:{}", AudioLimits::MAX_RENDER_FRAMES + 1),
    );
    assert!(from_wasapi(&args, true).is_err());
}

#[test]
fn asio_to_wasapi_uses_source_format_and_requires_clear_or_actual_endpoint() {
    for device in ["", "endpoint-B"] {
        let args = strings(&["--backend", "wasapi", "--device", device]);
        let settings::TargetPlan::Wasapi(plan) =
            settings::plan_target(&asio_cap(), None, source(), None, &args, true).unwrap()
        else {
            panic!("selected WASAPI did not route to WASAPI")
        };
        assert_eq!(
            plan.device.as_ref().map(|id| id.0.as_str()),
            if device.is_empty() {
                None
            } else {
                Some(device)
            }
        );
        assert_eq!(plan.format.sample_rate(), 48_000);
        assert_eq!(plan.format.channels(), 2);
        assert_eq!(plan.format.encoding(), SampleEncoding::Float32);
        assert_eq!(
            plan.mode,
            AudioStreamMode::Shared(SharedPeriodPolicy::EnginePeriod)
        );
        let resolved = plan
            .into_request(Some(AudioDeviceId("os-default".into())))
            .unwrap();
        assert_eq!(
            resolved.native.device().0,
            if device.is_empty() {
                "os-default"
            } else {
                device
            }
        );
    }
    let settings::TargetPlan::Wasapi(unresolved) = settings::plan_target(
        &asio_cap(),
        None,
        source(),
        None,
        &strings(&["--backend", "wasapi"]),
        true,
    )
    .unwrap() else {
        panic!("expected unresolved default WASAPI")
    };
    assert!(
        unresolved.into_request(None).is_err(),
        "OS default requires a real discovered endpoint"
    );
    let mut copied = asio_args();
    set(&mut copied, "--backend", "wasapi");
    assert!(
        settings::plan_target(&asio_cap(), None, source(), None, &copied, true).is_err(),
        "copied ASIO CLSID is not a WASAPI endpoint"
    );
}

#[test]
fn wasapi_target_ignores_inactive_asio_metadata_even_without_sdk() {
    let mut args = asio_args();
    set(&mut args, "--backend", "wasapi");
    set(&mut args, "--device", "endpoint-B");
    for (flag, value) in [
        ("--asio-view", "invalid-inactive"),
        ("--output-channels", "not-channels"),
        ("--asio-system-clock", "not-clock"),
        ("--asio-anchor-age-ns", "not-age"),
    ] {
        set(&mut args, flag, value);
    }
    let settings::TargetPlan::Wasapi(plan) = from_wasapi(&args, false).unwrap() else {
        panic!("inactive ASIO metadata changed WASAPI dispatch")
    };
    let resolved = plan.into_request(None).unwrap();
    assert_eq!(resolved.native.device().0, "endpoint-B");
    assert_eq!(resolved.native.buffer(), BufferRequest::Frames(257));
    assert_eq!(resolved.native.negotiation(), wasapi().negotiation());
}

#[test]
fn channel_remix_retains_original_pcm_columns_across_both_targets() {
    let matrix = ChannelMatrix::new(2, 1, &[0.5, 0.25]).unwrap();
    let mut args = asio_args();
    set(&mut args, "--output-channels", "3");
    set(&mut args, "--output-matrix", "0.5,0.25");
    let settings::TargetPlan::Asio(plan) =
        settings::plan_target(&wasapi_cap(), Some(&wasapi()), source(), None, &args, true).unwrap()
    else {
        panic!("expected ASIO")
    };
    assert_eq!(plan.matrix, Some(matrix.clone()));
    let mut mono_cap = asio_cap();
    set(&mut mono_cap.current_args, "--output-channels", "3");
    set(&mut mono_cap.current_args, "--output-matrix", "0.5,0.25");
    let settings::TargetPlan::Wasapi(back) = settings::plan_target(
        &mono_cap,
        None,
        source(),
        Some(&matrix),
        &strings(&["--backend", "wasapi", "--device", ""]),
        true,
    )
    .unwrap() else {
        panic!("expected WASAPI")
    };
    assert_eq!(back.matrix, Some(matrix));
    assert_eq!(back.format.channels(), 1);
    assert_eq!(back.format.sample_rate(), 48_000);
    set(&mut args, "--output-matrix", "1");
    assert!(
        from_wasapi(&args, true).is_err(),
        "one column cannot replace stereo source"
    );
    set(&mut args, "--output-matrix", "exact");
    assert!(
        from_wasapi(&args, true).is_err(),
        "one selected driver channel cannot receive exact stereo"
    );
}

#[test]
fn asio_registration_resolution_rejects_absent_and_ambiguous_selected_identity() {
    assert_eq!(
        settings::asio_driver_index(DRIVER, ["other", DRIVER].into_iter()).unwrap(),
        1
    );
    assert!(settings::asio_driver_index(DRIVER, ["other"].into_iter()).is_err());
    assert!(settings::asio_driver_index(DRIVER, [DRIVER, DRIVER].into_iter()).is_err());
}

#[test]
fn capability_arguments_roundtrip_through_target_planner() {
    let cap = asio_cap();
    let settings::TargetPlan::Asio(plan) =
        settings::plan_target(&cap, None, source(), None, &cap.current_args, true).unwrap()
    else {
        panic!("ASIO capability roundtrip changed backend")
    };
    assert_eq!(plan.device, DRIVER);
    assert_eq!(plan.channels, [3, 1]);
    assert_eq!(plan.buffer, AsioBufferRequest::Frames(257));
    let cap = wasapi_cap();
    let settings::TargetPlan::Wasapi(plan) = settings::plan_target(
        &cap,
        Some(&wasapi()),
        source(),
        None,
        &cap.current_args,
        false,
    )
    .unwrap() else {
        panic!("WASAPI capability roundtrip changed backend")
    };
    assert_eq!(plan.into_request(None).unwrap().native, wasapi());
}
