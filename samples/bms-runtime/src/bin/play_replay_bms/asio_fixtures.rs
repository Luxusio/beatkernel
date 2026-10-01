//! Portable CLI authoring fixtures; no backend, SDK or file IO is invoked.
use super::*;

const LOWER_ID: &str = "{12345678-9abc-def0-1234-56789abcdef0}";
const UPPER_ID: &str = "{12345678-9ABC-DEF0-1234-56789ABCDEF0}";
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
fn base() -> Vec<String> {
    args(&[
        "--chart",
        "unopened-score.bms",
        "--replay",
        "unopened-session.bkr",
        "--device",
        "endpoint-id",
        "--seconds",
        "1",
        "--rate",
        "48000",
        "--channels",
        "2",
    ])
}
fn set(arguments: &mut [String], flag: &str, value: &str) {
    let index = arguments
        .iter()
        .position(|argument| argument == flag)
        .expect("fixture flag exists");
    arguments[index + 1] = value.to_owned();
}
fn append(arguments: &mut Vec<String>, flag: &str, value: &str) {
    arguments.extend(args(&[flag, value]));
}
fn asio() -> Vec<String> {
    let mut arguments = base();
    set(&mut arguments, "--device", LOWER_ID);
    append(&mut arguments, "--backend", "asio");
    append(&mut arguments, "--asio-view", "native");
    append(&mut arguments, "--output-channels", "0,1");
    for (flag, value) in [
        ("--asio-system-clock", "multimedia"),
        ("--asio-timer-error-ns", "0"),
        ("--asio-drift-error-ns", "0"),
        ("--asio-latency-error-ns", "0"),
    ] {
        append(&mut arguments, flag, value);
    }
    arguments
}
fn native(host: Backend) -> Vec<String> {
    let mut arguments = base();
    match host {
        Backend::Linux => {
            append(&mut arguments, "--buffer-frames", "256");
            append(&mut arguments, "--period-frames", "64");
        }
        Backend::Macos => {
            set(&mut arguments, "--device", "42");
            append(&mut arguments, "--buffer-frames", "256");
        }
        _ => {}
    }
    arguments
}

#[test]
fn explicit_asio_routes_identity_view_and_channel_order_without_sdk_or_file_io() {
    let mut arguments = asio();
    set(&mut arguments, "--output-channels", "3,1");
    let options = parse(&arguments, Backend::Windows).unwrap();
    assert_eq!(options.backend, Backend::Asio);
    assert_eq!(options.device, UPPER_ID);
    assert!(matches!(options.asio_view, Some(AsioView::Native)));
    assert_eq!(options.output_channels, Some(vec![3, 1]));
    assert_eq!(options.buffer, None); // Driver-preferred, never an invented frame count.
    assert_eq!(options.format, AudioFormat::new(48000, 2).unwrap());
    assert_eq!(options.chart, PathBuf::from("unopened-score.bms"));
    assert_eq!(options.replay, PathBuf::from("unopened-session.bkr"));
    for (view, expected) in [("32", AsioView::Bits32), ("64", AsioView::Bits64)] {
        set(&mut arguments, "--asio-view", view);
        let options = parse(&arguments, Backend::Windows).unwrap();
        match expected {
            AsioView::Bits32 => assert!(matches!(options.asio_view, Some(AsioView::Bits32))),
            AsioView::Bits64 => assert!(matches!(options.asio_view, Some(AsioView::Bits64))),
            _ => unreachable!(),
        }
    }
}

#[test]
fn exact_positive_buffer_and_signed_native_channel_index_boundaries_are_preserved() {
    let mut arguments = asio();
    append(&mut arguments, "--buffer-frames", "257");
    set(&mut arguments, "--output-channels", "0,2147483647");
    let options = parse(&arguments, Backend::Windows).unwrap();
    assert_eq!(options.buffer, Some(257));
    assert_eq!(options.output_channels, Some(vec![0, 2_147_483_647]));
    set(
        &mut arguments,
        "--buffer-frames",
        &AudioLimits::MAX_RENDER_FRAMES.to_string(),
    );
    assert_eq!(
        parse(&arguments, Backend::Windows).unwrap().buffer,
        Some(AudioLimits::MAX_RENDER_FRAMES as u32)
    );
    for value in [
        "0".to_owned(),
        "-1".to_owned(),
        "4294967296".to_owned(),
        (AudioLimits::MAX_RENDER_FRAMES + 1).to_string(),
    ] {
        set(&mut arguments, "--buffer-frames", &value);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
}

#[test]
fn omitted_backend_keeps_host_default_and_explicit_host_backend_never_enables_asio_flags() {
    for (host, name) in [
        (Backend::Windows, "wasapi"),
        (Backend::Linux, "alsa"),
        (Backend::Macos, "coreaudio"),
    ] {
        let arguments = native(host);
        let options = parse(&arguments, host).unwrap();
        assert_eq!(options.backend, host);
        assert!(options.asio_view.is_none());
        assert!(options.output_channels.is_none());
        let mut explicit = arguments.clone();
        append(&mut explicit, "--backend", name);
        assert_eq!(parse(&explicit, host).unwrap().backend, host);
        for (flag, value) in [("--asio-view", "native"), ("--output-channels", "0,1")] {
            let mut invalid = explicit.clone();
            append(&mut invalid, flag, value);
            assert!(parse(&invalid, host).is_err());
        }
    }
}

#[test]
fn wrong_host_or_backend_and_asio_inapplicable_options_are_explicit_errors() {
    for host in [
        Backend::Linux,
        Backend::Macos,
        Backend::Unsupported,
        Backend::Asio,
    ] {
        assert!(parse(&asio(), host).is_err());
    }
    for (host, requested) in [
        (Backend::Windows, "alsa"),
        (Backend::Windows, "coreaudio"),
        (Backend::Linux, "wasapi"),
        (Backend::Linux, "coreaudio"),
        (Backend::Macos, "wasapi"),
        (Backend::Macos, "alsa"),
    ] {
        let mut arguments = native(host);
        append(&mut arguments, "--backend", requested);
        assert!(parse(&arguments, host).is_err());
    }
    for (flag, value) in [
        ("--mode", "shared"),
        ("--mode", "exclusive"),
        ("--shared-policy", "engine-period"),
        ("--shared-policy", "legacy"),
        ("--period-frames", "64"),
    ] {
        let mut arguments = asio();
        append(&mut arguments, flag, value);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
}

#[test]
fn output_routing_rejects_duplicates_negative_overflow_empty_tokens_and_count_mismatch() {
    for value in [
        "",
        "0",
        "0,1,2",
        "0,0",
        "1,1",
        "-1,0",
        "0,-1",
        "0,2147483648",
        "0,4294967295",
        "0,4294967296",
        "0,",
        ",1",
        "0,,1",
        "zero,1",
        "0,1e3",
    ] {
        let mut arguments = asio();
        set(&mut arguments, "--output-channels", value);
        assert!(
            parse(&arguments, Backend::Windows).is_err(),
            "routing {value:?}"
        );
    }
    let mut one = asio();
    set(&mut one, "--channels", "1");
    set(&mut one, "--output-channels", "0");
    assert_eq!(
        parse(&one, Backend::Windows).unwrap().output_channels,
        Some(vec![0])
    );
}

#[test]
fn strict_braced_uuid_shape_rejects_malformed_nonascii_nul_and_trailing_identity() {
    for device in [
        "",
        "12345678-9abc-def0-1234-56789abcdef0",
        "{12345678-9abc-def0-1234-56789abcdef}",
        "[12345678-9abc-def0-1234-56789abcdef0]",
        "{12345678_9abc-def0-1234-56789abcdef0}",
        "{g2345678-9abc-def0-1234-56789abcdef0}",
        "{12345678-9abc-def0-1234-56789abcdeg0}",
        "{é2345678-9abc-def0-1234-56789abcdef0}",
        "{12345678-9abc-def0-1234-56789abc\0ef0}",
        " {12345678-9abc-def0-1234-56789abcdef0}",
        "{12345678-9abc-def0-1234-56789abcdef0} ",
        "{{12345678-9abc-def0-1234-56789abcdef0}}",
    ] {
        let mut arguments = asio();
        set(&mut arguments, "--device", device);
        assert!(
            parse(&arguments, Backend::Windows).is_err(),
            "identity {device:?}"
        );
    }
    let mut uppercase = asio();
    set(&mut uppercase, "--device", UPPER_ID);
    assert_eq!(
        parse(&uppercase, Backend::Windows).unwrap().device,
        UPPER_ID
    );
}

#[test]
fn mandatory_asio_selection_and_duplicate_flags_never_choose_a_fallback() {
    for flag in [
        "--asio-view",
        "--output-channels",
        "--asio-system-clock",
        "--asio-timer-error-ns",
        "--asio-drift-error-ns",
        "--asio-latency-error-ns",
    ] {
        let mut arguments = asio();
        let index = arguments
            .iter()
            .position(|argument| argument == flag)
            .unwrap();
        arguments.drain(index..index + 2);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
    for (flag, value) in [
        ("--backend", "asio"),
        ("--asio-view", "native"),
        ("--output-channels", "0,1"),
        ("--device", UPPER_ID),
        ("--channels", "2"),
    ] {
        let mut arguments = asio();
        append(&mut arguments, flag, value);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
    let mut missing_value = asio();
    missing_value.push("--buffer-frames".to_owned());
    assert!(parse(&missing_value, Backend::Windows).is_err());
}

#[test]
fn backend_view_format_and_finite_capacity_values_are_strictly_validated() {
    for backend in ["", "ASIO", "windows", "auto", "unknown"] {
        let mut arguments = asio();
        set(&mut arguments, "--backend", backend);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
    for view in ["", "Native", "auto", "x86", "x64", "0", "128"] {
        let mut arguments = asio();
        set(&mut arguments, "--asio-view", view);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
    for (flag, value) in [
        ("--rate", "0"),
        ("--channels", "0"),
        ("--channels", "33"),
        ("--channels", "65536"),
    ] {
        let mut arguments = asio();
        set(&mut arguments, flag, value);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
    for (flag, value) in [
        ("--command-capacity", "0"),
        ("--voices", "0"),
        ("--max-records", "0"),
        ("--max-bytes", "1"),
        ("--lookahead-ns", "0"),
        ("--preroll-ns", "-1"),
    ] {
        let mut arguments = asio();
        append(&mut arguments, flag, value);
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
    for flag in ["--command-capacity", "--voices"] {
        let mut arguments = asio();
        append(&mut arguments, flag, &usize::MAX.to_string());
        assert!(parse(&arguments, Backend::Windows).is_err());
    }
}

#[test]
fn recorded_asio_natural_completion_requires_explicit_bounded_clock_assessments() {
    let mut natural = asio();
    let index = natural.iter().position(|s| s == "--seconds").unwrap();
    natural.drain(index..index + 2);
    let options = parse(&natural, Backend::Windows).unwrap();
    assert_eq!(options.seconds, None);
    assert_eq!(options.asio_clock.unwrap().anchor_age, 1_000_000_000);
    for flag in [
        "--asio-timer-error-ns",
        "--asio-drift-error-ns",
        "--asio-latency-error-ns",
    ] {
        for value in ["", "-1", "+1", "NaN", "9223372036854775808"] {
            let mut invalid = natural.clone();
            set(&mut invalid, flag, value);
            assert!(parse(&invalid, Backend::Windows).is_err());
        }
    }
    for age in ["0", "2147483648000000", "18446744073709551615"] {
        let mut invalid = natural.clone();
        append(&mut invalid, "--asio-anchor-age-ns", age);
        assert!(parse(&invalid, Backend::Windows).is_err());
    }
    let mut incompatible = natural.clone();
    set(&mut incompatible, "--asio-system-clock", "qpc");
    assert!(parse(&incompatible, Backend::Windows).is_err());
    for (flag, value) in [
        ("--asio-system-clock", "multimedia"),
        ("--asio-timer-error-ns", "0"),
        ("--asio-drift-error-ns", "0"),
        ("--asio-latency-error-ns", "0"),
        ("--asio-anchor-age-ns", "1"),
    ] {
        let mut wrong_backend = native(Backend::Windows);
        append(&mut wrong_backend, flag, value);
        assert!(parse(&wrong_backend, Backend::Windows).is_err());
        let mut duplicate = natural.clone();
        append(&mut duplicate, flag, value);
        if flag != "--asio-anchor-age-ns" {
            assert!(parse(&duplicate, Backend::Windows).is_err());
        }
    }
}
