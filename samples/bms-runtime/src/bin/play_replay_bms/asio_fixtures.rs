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

mod playback_control {
    use super::*;
    use beatkernel::audio::{AudioCommand, CommandProducer, PcmSample, SampleBank, SampleId, VoiceId};
    use beatkernel_bms_runtime::native_start::interval::StartInterval;

    fn point(domain: ClockDomainId, ns: i64) -> ClockPoint {
        ClockPoint {
            domain,
            timestamp: Timestamp::from_nanos(ns),
        }
    }
    fn fixture() -> (CommandProducer, Mixer, ReplayPause) {
        let format = AudioFormat::new(1000, 1).unwrap();
        let limits = AudioLimits::new(8, 2, 8, 32, 8).unwrap();
        let pcm = PcmLimits::new(256, 512, 1).unwrap();
        let mut bank = SampleBank::new(format, pcm).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(
                format,
                (1..=64).map(|value| value as f32 / 256.0).collect(),
                pcm,
            )
            .unwrap(),
        )
        .unwrap();
        let (mut producer, consumer) = command_queue(8).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
        let mixer = Mixer::new(
            MixerConfig::new(format, OUTPUT, Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap();
        let pause = ReplayPause::new(
            point(OUTPUT, 0),
            HOST,
            1000,
            Timestamp::from_nanos(50_000_000),
            Duration::from_nanos(3_000_000),
        )
        .unwrap();
        (producer, mixer, pause)
    }
    fn observed(render: RenderReport, before: i64, after: i64) -> PauseIntervalObservation {
        PauseIntervalObservation {
            output_origin: point(OUTPUT, 0),
            sample_rate: 1000,
            render,
            clock: StartInterval::new(
                point(OUTPUT, render.start_frame as i64 * 1_000_000),
                point(HOST, before),
                point(HOST, after),
            )
            .unwrap(),
        }
    }
    fn interval(
        observation: Option<PauseIntervalObservation>,
        now: i64,
    ) -> Option<PauseObservation> {
        Some(PauseObservation::Interval {
            observation,
            now: point(HOST, now),
        })
    }
    fn step(
        pause: &mut ReplayPause,
        available: &mut bool,
        evidence: Option<PauseObservation>,
        rendered: Option<RenderReport>,
        desired: bool,
        producer: &mut CommandProducer,
        requests: &mut Vec<bool>,
    ) -> Result<ReplayPauseUpdate> {
        update_replay_pause(pause, available, evidence, rendered, desired, |paused| {
            requests.push(paused);
            producer.request_pause(paused);
        })
    }
    struct FakeOutput {
        pair: Option<ClockPair>,
        source: Option<ClockPoint>,
        report: Option<RenderReport>,
        source_reads: usize,
        pair_reads: usize,
    }
    impl NativeOutput for FakeOutput {
        fn start(&mut self) -> Result<()> {
            Ok(())
        }
        fn stop(&mut self) -> Result<()> {
            Ok(())
        }
        fn poll(&mut self) -> Result<Option<RenderReport>> {
            Ok(self.report)
        }
        fn presented(&mut self) -> Result<Option<ClockPoint>> {
            self.source_reads += 1;
            Ok(self.source)
        }
        fn presentation_pair(&mut self) -> Result<Option<ClockPair>> {
            self.pair_reads += 1;
            Ok(self.pair)
        }
        fn last_render(&mut self) -> Option<RenderReport> {
            self.report
        }
        fn final_check(&mut self) -> Result<()> {
            Ok(())
        }
        fn print_native(&mut self) {}
    }

    #[test]
    fn absent_relations_and_source_only_presentation_cannot_enable_or_request_pause() {
        let (mut producer, mut mixer, mut pause) = fixture();
        let report = mixer.render(&mut [0.0; 4]).unwrap();
        let mut available = false;
        let mut requests = Vec::new();
        for evidence in [None, interval(None, 120)] {
            let update = step(
                &mut pause,
                &mut available,
                evidence,
                Some(report),
                true,
                &mut producer,
                &mut requests,
            )
            .unwrap();
            assert!(!update.became_available);
            assert_eq!(update.requested, None);
            assert!(update.boundary.is_none());
            assert!(!available);
            assert_eq!(pause.phase(), PausePhase::Running);
        }
        let mut output = FakeOutput {
            pair: None,
            source: Some(point(OUTPUT, 4_000_000)),
            report: Some(report),
            source_reads: 0,
            pair_reads: 0,
        };
        let frame = output.presentation().unwrap();
        assert_eq!(frame.presented, Some(point(OUTPUT, 4_000_000)));
        assert!(frame.pause.is_none());
        assert_eq!((output.pair_reads, output.source_reads), (1, 1));
        step(
            &mut pause,
            &mut available,
            frame.pause,
            output.poll().unwrap(),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(!available && requests.is_empty());
        assert_eq!(
            pause.presentation_song(frame.presented.unwrap()).unwrap(),
            Some(Timestamp::from_nanos(51_000_000))
        );
        let mut sound = [99.0; 2];
        assert!(!mixer.render(&mut sound).unwrap().paused);
        assert_eq!(sound, [5.0 / 256.0, 6.0 / 256.0]);
    }

    #[test]
    fn common_interval_step_routes_requests_once_and_acks_without_new_native_evidence() {
        let (mut producer, mut mixer, mut pause) = fixture();
        let first = observed(mixer.render(&mut [0.0; 4]).unwrap(), 100, 120);
        let mut available = false;
        let mut requests = Vec::new();
        let update = step(
            &mut pause,
            &mut available,
            interval(Some(first), 120),
            Some(first.render),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(update.became_available && available);
        assert_eq!(update.requested, Some(true));
        assert!(update.boundary.is_none());
        assert_eq!(pause.phase(), PausePhase::Pausing);
        let repeated = step(
            &mut pause,
            &mut available,
            interval(Some(first), 120),
            Some(first.render),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(!repeated.became_available);
        assert_eq!(repeated.requested, None);
        assert_eq!(requests, [true]);
        let mut silence = [99.0; 3];
        let frozen = observed(mixer.render(&mut silence).unwrap(), 200, 240);
        assert_eq!(silence, [0.0; 3]);
        let later_poll = observed(mixer.render(&mut [99.0; 2]).unwrap(), 300, 340);
        assert_ne!(frozen.render.start_frame, later_poll.render.start_frame);
        let waiting = step(
            &mut pause,
            &mut available,
            interval(Some(frozen), 239),
            Some(later_poll.render),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(waiting.boundary.is_none());
        assert_eq!(pause.last_render_report(), Some(frozen.render));
        assert_eq!(
            pause.presentation_song(point(OUTPUT, 7_000_000)).unwrap(),
            None
        );
        let paused = step(
            &mut pause,
            &mut available,
            interval(None, 240),
            Some(later_poll.render),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert!(paused.paused);
        assert_eq!(paused.song, Timestamp::from_nanos(51_000_000));
        assert_eq!(paused.interval.unwrap().earliest(), point(HOST, 200));
        assert_eq!(paused.interval.unwrap().latest(), point(HOST, 240));
        assert_eq!(pause.phase(), PausePhase::Paused);
        assert_eq!(requests, [true]);

        let resume = step(
            &mut pause,
            &mut available,
            interval(Some(later_poll), 340),
            Some(later_poll.render),
            false,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert_eq!(resume.requested, Some(false));
        assert!(resume.boundary.is_none());
        assert_eq!(pause.phase(), PausePhase::Resuming);
        let mut sound = [99.0; 2];
        let resumed = observed(mixer.render(&mut sound).unwrap(), 400, 440);
        assert_eq!(sound, [5.0 / 256.0, 6.0 / 256.0]);
        assert_eq!(
            (
                resumed.render.start_frame,
                resumed.render.playback_start_frame
            ),
            (9, 4)
        );
        // A newer independently polled report must not become interval evidence.
        let absent = step(
            &mut pause,
            &mut available,
            interval(None, 400),
            Some(resumed.render),
            false,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(absent.boundary.is_none());
        assert_eq!(pause.phase(), PausePhase::Resuming);
        let waiting = step(
            &mut pause,
            &mut available,
            interval(Some(resumed), 439),
            Some(resumed.render),
            false,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(waiting.boundary.is_none());
        let running = step(
            &mut pause,
            &mut available,
            interval(None, 440),
            None,
            false,
            &mut producer,
            &mut requests,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert!(!running.paused);
        assert_eq!(running.song, paused.song);
        assert_eq!(running.interval.unwrap().latest(), point(HOST, 440));
        assert_eq!(pause.phase(), PausePhase::Running);
        assert_eq!(requests, [true, false]);
        assert_eq!(
            pause.presentation_song(point(OUTPUT, 8_999_999)).unwrap(),
            None
        );
        assert_eq!(
            pause.presentation_song(point(OUTPUT, 9_000_000)).unwrap(),
            Some(paused.song)
        );
    }

    #[test]
    fn invalid_control_evidence_never_requests_audio_or_loses_a_pending_acknowledgement() {
        let (mut producer, mut mixer, mut pause) = fixture();
        let first = observed(mixer.render(&mut [0.0; 4]).unwrap(), 100, 120);
        let mut available = false;
        let mut requests = Vec::new();
        let bad_rate = PauseIntervalObservation {
            sample_rate: 0,
            ..first
        };
        for evidence in [
            Some(PauseObservation::Interval {
                observation: Some(first),
                now: point(ClockDomainId(9), 120),
            }),
            interval(Some(bad_rate), 120),
        ] {
            assert!(
                step(
                    &mut pause,
                    &mut available,
                    evidence,
                    Some(first.render),
                    true,
                    &mut producer,
                    &mut requests
                )
                .is_err()
            );
            assert!(!available && requests.is_empty());
            assert_eq!(pause.phase(), PausePhase::Running);
            assert_eq!(pause.last_render_report(), None);
        }
        step(
            &mut pause,
            &mut available,
            interval(Some(first), 120),
            Some(first.render),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        let frozen = observed(mixer.render(&mut [0.0; 2]).unwrap(), 200, 240);
        let wrong_now = Some(PauseObservation::Interval {
            observation: Some(frozen),
            now: point(ClockDomainId(9), 1000),
        });
        assert!(
            step(
                &mut pause,
                &mut available,
                wrong_now,
                Some(frozen.render),
                true,
                &mut producer,
                &mut requests
            )
            .is_err()
        );
        assert_eq!(pause.phase(), PausePhase::Pausing);
        assert_eq!(pause.last_render_report(), Some(first.render));
        assert_eq!(requests, [true]);
        step(
            &mut pause,
            &mut available,
            interval(Some(frozen), 239),
            Some(frozen.render),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(
            step(
                &mut pause,
                &mut available,
                interval(None, 238),
                None,
                true,
                &mut producer,
                &mut requests
            )
            .is_err()
        );
        assert_eq!(pause.phase(), PausePhase::Pausing);
        let boundary = step(
            &mut pause,
            &mut available,
            interval(None, 240),
            None,
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert_eq!(boundary.song, Timestamp::from_nanos(51_000_000));
        assert_eq!(boundary.interval.unwrap().latest(), point(HOST, 240));
        assert_eq!(requests, [true]);
    }

    #[test]
    fn point_output_default_keeps_existing_pause_flow_and_uses_the_same_resume_floor() {
        let (mut producer, mut mixer, mut pause) = fixture();
        let first = mixer.render(&mut [0.0; 4]).unwrap();
        let mut output = FakeOutput {
            pair: Some(ClockPair {
                source: point(OUTPUT, 0),
                target: point(HOST, 100),
            }),
            source: Some(point(OUTPUT, 99_000_000)),
            report: Some(first),
            source_reads: 0,
            pair_reads: 0,
        };
        let mut available = false;
        let mut requests = Vec::new();
        let frame = output.presentation().unwrap();
        assert_eq!(frame.presented, Some(point(OUTPUT, 0)));
        assert_eq!(output.source_reads, 0);
        let requested = step(
            &mut pause,
            &mut available,
            frame.pause,
            output.poll().unwrap(),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert!(requested.became_available);
        assert_eq!(requested.requested, Some(true));
        let frozen = mixer.render(&mut [0.0; 3]).unwrap();
        output.report = Some(frozen);
        output.pair = Some(ClockPair {
            source: point(OUTPUT, 3_000_000),
            target: point(HOST, 300),
        });
        let frame = output.presentation().unwrap();
        assert!(
            step(
                &mut pause,
                &mut available,
                frame.pause,
                output.poll().unwrap(),
                true,
                &mut producer,
                &mut requests
            )
            .unwrap()
            .boundary
            .is_none()
        );
        output.pair = Some(ClockPair {
            source: point(OUTPUT, 4_000_000),
            target: point(HOST, 400),
        });
        let frame = output.presentation().unwrap();
        let paused = step(
            &mut pause,
            &mut available,
            frame.pause,
            output.poll().unwrap(),
            true,
            &mut producer,
            &mut requests,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert!(paused.paused && paused.interval.is_none());
        assert_eq!(paused.song, Timestamp::from_nanos(51_000_000));
        let resumed = step(
            &mut pause,
            &mut available,
            frame.pause,
            output.poll().unwrap(),
            false,
            &mut producer,
            &mut requests,
        )
        .unwrap();
        assert_eq!(resumed.requested, Some(false));
        output.report = Some(mixer.render(&mut [0.0; 2]).unwrap());
        output.pair = Some(ClockPair {
            source: point(OUTPUT, 7_000_000),
            target: point(HOST, 700),
        });
        let frame = output.presentation().unwrap();
        let running = step(
            &mut pause,
            &mut available,
            frame.pause,
            output.poll().unwrap(),
            false,
            &mut producer,
            &mut requests,
        )
        .unwrap()
        .boundary
        .unwrap();
        assert!(!running.paused && running.interval.is_none());
        assert_eq!(running.song, paused.song);
        assert_eq!(requests, [true, false]);
        assert_eq!(output.source_reads, 0);
        assert_eq!(
            pause.presentation_song(point(OUTPUT, 6_999_999)).unwrap(),
            None
        );
        assert_eq!(
            pause.presentation_song(frame.presented.unwrap()).unwrap(),
            Some(paused.song)
        );
    }
}
