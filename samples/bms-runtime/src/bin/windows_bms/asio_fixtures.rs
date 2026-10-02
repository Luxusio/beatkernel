//! Portable parser and native-evidence fixtures; no driver, clock, asset or replay IO.
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

#[test]
fn asio_finite_prefix_admits_exact_solo_or_local_end_without_network_or_sdk_io() {
    let mut arguments = asio();
    append(&mut arguments, "--start-ns", "1000000000");
    append(&mut arguments, "--end-ns", "1000000001");
    assert!(super::validate_args(&arguments).is_ok());
    let options = parse(&arguments).unwrap();
    assert_eq!(options.playback_end(48000).unwrap(), Some(144001));
    append(&mut arguments, "--local-player", "7:path-a");
    append(&mut arguments, "--local-player", "4294967295:path-b");
    assert!(super::validate_args(&arguments).is_ok());
    append(&mut arguments, "--mp-host", "127.0.0.1:39001");
    assert!(super::validate_args(&arguments).is_err());
    let mut solo_network = asio();
    append(&mut solo_network, "--end-ns", "1");
    append(&mut solo_network, "--mp-host", "127.0.0.1:39001");
    assert!(
        super::validate_args(&solo_network)
            .unwrap_err()
            .to_string()
            .contains("network")
    );
    for value in [
        "",
        "-1",
        "+1",
        "0",
        "1000000000",
        "1.5",
        "9223372036854775808",
    ] {
        let mut invalid = asio();
        append(&mut invalid, "--start-ns", "1000000000");
        append(&mut invalid, "--end-ns", value);
        assert!(super::validate_args(&invalid).is_err());
    }
    let mut duplicate = asio();
    append(&mut duplicate, "--end-ns", "1");
    append(&mut duplicate, "--end-ns", "2");
    assert!(super::validate_args(&duplicate).is_err());
}

#[test]
fn asio_actual_upper_interval_gates_windows_completion_after_render_and_message_drain() {
    use beatkernel::{
        audio::{
            AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue,
        },
        time::{ClockDomainId, ClockPoint, Timestamp},
    };
    use beatkernel_bms_runtime::native_end::NativeEnd;
    use beatkernel_platform::audio::asio::{AsioPresentationObservation, MultimediaHostInterval};
    let host = |ns| ClockPoint {
        domain: ClockDomainId(1),
        timestamp: Timestamp::from_nanos(ns),
    };
    let origin = ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::ZERO,
    };
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            origin.domain,
            origin.timestamp,
            AudioLimits::new(8, 1, 8, 16, 8).unwrap(),
        )
        .with_playback_end_frame(2),
        bank,
        consumer,
    )
    .unwrap();
    let observation = |report, switch| {
        AsioPresentationObservation::from_render(
            report,
            1000,
            MultimediaHostInterval {
                before: host(switch),
                after: host(switch + 100),
            },
            3,
            200,
            origin,
        )
        .unwrap()
    };
    let mut end = NativeEnd::new(origin, ClockDomainId(1), 1000, 2).unwrap();
    let first = mixer.render(&mut [0.0]).unwrap();
    assert!(end.observe_asio(observation(first, 0)).unwrap().is_none());
    let terminal = mixer.render(&mut [0.0; 2]).unwrap();
    assert_eq!(terminal.playback_end_physical_frame, Some(2));
    assert!(
        end.observe_asio(observation(terminal, 1_000_000))
            .unwrap()
            .is_none()
    );
    let silence = mixer.render(&mut [1.0; 2]).unwrap();
    let observed = observation(silence, 3_000_000);
    let boundary = end.observe_asio(observed).unwrap().unwrap();
    assert_eq!(boundary.host, observed.host.after);
    assert_eq!(boundary.host, host(6_000_300));
    let song = Timestamp::from_nanos(2_000_000);
    for (watermark, backlog, logical) in [
        (host(6_000_299), false, song),
        (host(6_000_300), true, song),
        (host(6_000_300), false, Timestamp::from_nanos(1_999_999)),
    ] {
        assert!(!super::finite_session_done(
            Some(2_000_000),
            Some(boundary.host),
            watermark,
            logical,
            backlog,
            false
        ));
    }
    assert!(super::finite_session_done(
        Some(2_000_000),
        Some(boundary.host),
        host(6_000_300),
        song,
        false,
        false
    ));
}

#[test]
fn actual_asio_pause_conversion_preserves_the_original_interval_report_and_fresh_host() {
    use beatkernel::{
        audio::{
            AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue,
        },
        time::{ClockDomainId, ClockPoint, Timestamp},
    };
    use beatkernel_bms_runtime::live_pause::LivePauseObservation;
    use beatkernel_platform::audio::asio::{AsioPresentationObservation, MultimediaHostInterval};
    let point = |domain, ns| ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    };
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 1, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.0; 2]).unwrap();
    producer.request_pause(true);
    let mut silence = [1.0; 2];
    let render = mixer.render(&mut silence).unwrap();
    assert_eq!(silence, [0.0; 2]);
    assert_eq!(
        (
            render.start_frame,
            render.playback_start_frame,
            render.playback_frames
        ),
        (2, 2, 0)
    );
    let original = AsioPresentationObservation::from_render(
        render,
        1000,
        MultimediaHostInterval {
            before: point(1, 5_000_000),
            after: point(1, 6_000_000),
        },
        3,
        200,
        point(2, 0),
    )
    .unwrap();
    let LivePauseObservation::Interval {
        observation: Some(observed),
        now,
    } = super::asio_pause_observation(Some(original), point(1, 10_000_000))
    else {
        panic!("ASIO must retain interval evidence");
    };
    assert_eq!(observed.output_origin, point(2, 0));
    assert_eq!(observed.sample_rate, 1000);
    assert_eq!(observed.render, render);
    assert_eq!(observed.clock.output, point(2, 2_000_000));
    assert_eq!(observed.clock.before, point(1, 7_999_800));
    assert_eq!(observed.clock.after, point(1, 9_000_200));
    assert_eq!(now, point(1, 10_000_000));
    let LivePauseObservation::Interval { observation, now } =
        super::asio_pause_observation(None, point(1, 11_000_000))
    else {
        panic!("missing ASIO telemetry must retain its evidence mode");
    };
    assert!(observation.is_none());
    assert_eq!(now, point(1, 11_000_000));
}

#[test]
fn native_asio_resume_reseeds_genuine_evidence_when_old_midpoint_has_not_progressed() {
    use beatkernel::{
        audio::{
            AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits, SampleBank, command_queue,
        },
        time::{ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp},
    };
    use beatkernel_platform::audio::{
        asio::{AsioPresentationObservation, MultimediaHostInterval},
        presentation::discipline::{
            DisciplineConfig, ObservationAdmission, PresentationDiscipline,
        },
    };
    let point = |domain, ns| ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    };
    let format = AudioFormat::new(1000, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (_producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(8, 1, 8, 16, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let observation = |render| {
        AsioPresentationObservation::from_render(
            render,
            1000,
            MultimediaHostInterval {
                before: point(1, 10_000_000),
                after: point(1, 14_000_000),
            },
            0,
            0,
            point(2, 0),
        )
        .unwrap()
    };
    let first = observation(mixer.render(&mut [0.0; 2]).unwrap());
    let latest = observation(mixer.render(&mut [0.0; 2]).unwrap());
    let new_discipline = || {
        PresentationDiscipline::new(
            DisciplineConfig::default(),
            point(2, 0),
            ClockDomainId(1),
            Timestamp::ZERO,
        )
        .unwrap()
    };
    let mut old = new_discipline();
    assert_eq!(
        old.observe_asio(first).unwrap(),
        ObservationAdmission::Retained
    );
    let stale_reference = old.latest_pair().unwrap();
    assert_eq!(
        old.observe_asio(latest).unwrap(),
        ObservationAdmission::AwaitingHostProgress
    );
    assert_eq!(old.latest_pair(), Some(stale_reference));

    let mut resumed = new_discipline();
    super::seed_asio_resume(&mut resumed, latest).unwrap();
    assert_eq!(
        resumed.latest_pair(),
        Some(ClockPair {
            source: point(2, 2_000_000),
            target: point(1, 12_000_000)
        })
    );
    assert_ne!(resumed.latest_pair(), Some(stale_reference));
    assert_eq!(resumed.quality(), ClockMappingQuality::Unknown);
    // Seeding retained ASIO source identity, rather than a fabricated supplied pair.
    assert_eq!(
        resumed.observe_asio(latest).unwrap(),
        ObservationAdmission::Unchanged
    );
    let mut invalid = latest;
    invalid.sample_rate = 999;
    let before = resumed.latest_pair();
    assert!(super::seed_asio_resume(&mut resumed, invalid).is_err());
    assert_eq!(resumed.latest_pair(), before);
}
