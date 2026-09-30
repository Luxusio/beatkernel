//! Portable parser fixtures; no Windows, driver, clock, asset or replay IO.
use super::parse;

const CLSID: &str = "{12345678-9abc-def0-1234-56789abcdef0}";
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
fn common() -> Vec<String> {
    strings(&[
        "--chart",
        "unopened.bms",
        "--device",
        "endpoint-id",
        "--seconds",
        "2",
        "--bind",
        "11:04",
    ])
}
fn append(arguments: &mut Vec<String>, flag: &str, value: &str) {
    arguments.extend(strings(&[flag, value]));
}
fn set(arguments: &mut [String], flag: &str, value: &str) {
    let index = arguments
        .iter()
        .position(|argument| argument == flag)
        .expect("fixture flag exists");
    arguments[index + 1] = value.to_owned();
}
fn wasapi() -> Vec<String> {
    let mut arguments = common();
    append(&mut arguments, "--mode", "shared");
    arguments
}
fn asio() -> Vec<String> {
    let mut arguments = common();
    set(&mut arguments, "--device", CLSID);
    for (flag, value) in [
        ("--backend", "asio"),
        ("--asio-view", "native"),
        ("--output-channels", "0,1"),
        ("--asio-system-clock", "multimedia"),
        ("--asio-timer-error-ns", "0"),
        ("--asio-drift-error-ns", "0"),
        ("--asio-latency-error-ns", "0"),
    ] {
        append(&mut arguments, flag, value);
    }
    arguments
}

#[test]
fn valid_asio_requires_explicit_clock_assessment_and_accepts_preferred_or_exact_frames() {
    assert!(parse(&asio()).is_ok()); // Portable parsing does not require the SDK feature.
    for buffer in ["default", "frames:1", "frames:257"] {
        let mut arguments = asio();
        append(&mut arguments, "--buffer", buffer);
        assert!(parse(&arguments).is_ok(), "buffer {buffer}");
    }
    for view in ["native", "32", "64"] {
        let mut arguments = asio();
        set(&mut arguments, "--asio-view", view);
        assert!(parse(&arguments).is_ok());
    }
    let mut uppercase = asio();
    set(
        &mut uppercase,
        "--device",
        "{12345678-9ABC-DEF0-1234-56789ABCDEF0}",
    );
    assert!(parse(&uppercase).is_ok());
}

#[test]
fn each_required_asio_field_is_mandatory_without_default_driver_or_clock_assessment() {
    for flag in [
        "--device",
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
        assert!(parse(&arguments).is_err(), "missing {flag}");
    }
}

#[test]
fn wasapi_default_and_explicit_backend_keep_required_mode_and_previous_period_rules() {
    assert!(parse(&wasapi()).is_ok());
    let mut explicit = wasapi();
    append(&mut explicit, "--backend", "wasapi");
    assert!(parse(&explicit).is_ok());
    assert!(parse(&common()).is_err());
    let mut exclusive = common();
    append(&mut exclusive, "--mode", "exclusive");
    append(&mut exclusive, "--buffer", "ns:10000000");
    append(&mut exclusive, "--period", "frames:128");
    assert!(parse(&exclusive).is_ok());
    let mut shared = wasapi();
    append(&mut shared, "--shared-policy", "legacy");
    append(&mut shared, "--period", "default");
    assert!(parse(&shared).is_ok());
    set(&mut shared, "--period", "frames:128");
    assert!(parse(&shared).is_err());
    append(&mut exclusive, "--shared-policy", "engine");
    assert!(parse(&exclusive).is_err());
}

#[test]
fn asio_rejects_every_explicit_wasapi_only_option_including_default_period() {
    for (flag, value) in [
        ("--mode", "shared"),
        ("--mode", "exclusive"),
        ("--period", "default"),
        ("--period", "frames:128"),
        ("--period", "ns:10000000"),
        ("--shared-policy", "engine"),
        ("--shared-policy", "legacy"),
        ("--buffer", "ns:10000000"),
    ] {
        let mut arguments = asio();
        append(&mut arguments, flag, value);
        assert!(parse(&arguments).is_err(), "{flag} {value}");
    }
}

#[test]
fn wasapi_rejects_asio_only_flags_instead_of_ignoring_them() {
    for (flag, value) in [
        ("--asio-view", "native"),
        ("--output-channels", "0,1"),
        ("--asio-system-clock", "multimedia"),
        ("--asio-timer-error-ns", "0"),
        ("--asio-drift-error-ns", "0"),
        ("--asio-latency-error-ns", "0"),
        ("--asio-anchor-age-ns", "1000000000"),
    ] {
        let mut arguments = wasapi();
        append(&mut arguments, flag, value);
        assert!(parse(&arguments).is_err(), "{flag}");
    }
}

#[test]
fn malformed_or_zero_driver_identity_unknown_views_and_clock_declarations_reject() {
    for id in [
        "",
        "12345678-9abc-def0-1234-56789abcdef0",
        "{12345678-9abc-def0-1234-56789abcdef}",
        "[12345678-9abc-def0-1234-56789abcdef0]",
        "{g2345678-9abc-def0-1234-56789abcdef0}",
        "{12345678_9abc-def0-1234-56789abcdef0}",
        "{é2345678-9abc-def0-1234-56789abcdef0}",
        "{12345678-9abc-def0-1234-56789abcdef0} ",
        "{12345678-9abc-def0-1234-56789abc\0ef0}",
        "{00000000-0000-0000-0000-000000000000}",
    ] {
        let mut arguments = asio();
        set(&mut arguments, "--device", id);
        assert!(parse(&arguments).is_err(), "{id:?}");
    }
    for view in ["", "auto", "Native", "x86", "x64", "128"] {
        let mut arguments = asio();
        set(&mut arguments, "--asio-view", view);
        assert!(parse(&arguments).is_err());
    }
    for clock in ["", "qpc", "auto", "Multimedia", "sample-position"] {
        let mut arguments = asio();
        set(&mut arguments, "--asio-system-clock", clock);
        assert!(parse(&arguments).is_err());
    }
}

#[test]
fn routing_is_distinct_nonempty_unsigned_and_bounded_to_thirty_two_outputs() {
    for routing in [
        "",
        "0,0",
        "-1,0",
        "0,-1",
        ",1",
        "0,",
        "0,,1",
        "0,4294967296",
        "left,right",
    ] {
        let mut arguments = asio();
        set(&mut arguments, "--output-channels", routing);
        assert!(parse(&arguments).is_err(), "{routing:?}");
    }
    for routing in ["0", "3,1", "0,1,2"] {
        let mut arguments = asio();
        set(&mut arguments, "--output-channels", routing);
        assert!(parse(&arguments).is_ok());
    }
    let exact = (0..32)
        .map(|channel| channel.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut arguments = asio();
    set(&mut arguments, "--output-channels", &exact);
    assert!(parse(&arguments).is_ok());
    set(&mut arguments, "--output-channels", &(exact + ",32"));
    assert!(parse(&arguments).is_err());
}

#[test]
fn error_assessments_and_anchor_age_have_explicit_nonnegative_and_modular_bounds() {
    for (flag, accepted) in [
        ("--asio-timer-error-ns", false),
        ("--asio-drift-error-ns", false),
        ("--asio-latency-error-ns", true),
    ] {
        let mut arguments = asio();
        set(&mut arguments, flag, "9223372036854775807");
        assert_eq!(parse(&arguments).is_ok(), accepted, "{flag} i64 maximum");
        for value in ["-1", "9223372036854775808", "18446744073709551616", "NaN"] {
            set(&mut arguments, flag, value);
            assert!(parse(&arguments).is_err(), "{flag} {value}");
        }
    }
    for age in ["1", "1000000000", "2147483647999999"] {
        let mut arguments = asio();
        append(&mut arguments, "--asio-anchor-age-ns", age);
        assert!(parse(&arguments).is_ok());
    }
    for age in [
        "0",
        "-1",
        "2147483648000000",
        "2147483648000001",
        "18446744073709551615",
    ] {
        let mut arguments = asio();
        append(&mut arguments, "--asio-anchor-age-ns", age);
        assert!(parse(&arguments).is_err());
    }
    // One nanosecond of either clock error consumes the last available modular
    // margin; the separate latency assessment does not consume that margin.
    for flag in ["--asio-timer-error-ns", "--asio-drift-error-ns"] {
        let mut arguments = asio();
        append(&mut arguments, "--asio-anchor-age-ns", "2147483647999999");
        set(&mut arguments, flag, "1");
        assert!(parse(&arguments).is_err());
    }
    let mut latency = asio();
    append(&mut latency, "--asio-anchor-age-ns", "2147483647999999");
    set(
        &mut latency,
        "--asio-latency-error-ns",
        "9223372036854775807",
    );
    assert!(parse(&latency).is_ok());
    // Each assessment fits with the default one-second horizon on its own;
    // together they reach half wrap even before adding that horizon.
    for flag in ["--asio-timer-error-ns", "--asio-drift-error-ns"] {
        let mut arguments = asio();
        set(&mut arguments, flag, "1073741824000000");
        assert!(parse(&arguments).is_ok());
    }
    let mut combined = asio();
    set(&mut combined, "--asio-timer-error-ns", "1073741824000000");
    set(&mut combined, "--asio-drift-error-ns", "1073741824000000");
    assert!(parse(&combined).is_err());
}

#[test]
fn duplicate_options_backend_aliases_unknown_flags_and_missing_values_reject() {
    for (flag, value) in [
        ("--backend", "asio"),
        ("--asio-view", "native"),
        ("--output-channels", "0,1"),
        ("--asio-system-clock", "multimedia"),
        ("--asio-timer-error-ns", "0"),
        ("--asio-drift-error-ns", "0"),
        ("--asio-latency-error-ns", "0"),
        ("--device", CLSID),
    ] {
        let mut arguments = asio();
        append(&mut arguments, flag, value);
        assert!(parse(&arguments).is_err());
    }
    for backend in ["", "windows", "ASIO", "auto", "alsa", "coreaudio"] {
        let mut arguments = asio();
        set(&mut arguments, "--backend", backend);
        assert!(parse(&arguments).is_err());
    }
    let mut unknown = asio();
    append(&mut unknown, "--asio-clock", "multimedia");
    assert!(parse(&unknown).is_err());
    let mut missing = asio();
    missing.push("--buffer".to_owned());
    assert!(parse(&missing).is_err());
    for buffer in [
        "frames:0",
        "frames:-1",
        "frames:4294967296",
        "frames:",
        "257",
        "preferred",
    ] {
        let mut arguments = asio();
        append(&mut arguments, "--buffer", buffer);
        assert!(parse(&arguments).is_err());
    }
}
