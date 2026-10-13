//! Independent recorded-output contracts over the parser and real cold request.
use super::*;
use beatkernel_bms_runtime::settings::OutputEncoding;

#[cfg(target_os = "linux")]
#[path = "../../../tests/support/replay_speed.rs"]
mod capture;

fn args(extra: &[&str]) -> Vec<String> {
    [
        "--chart",
        "unopened-score.bms",
        "--replay",
        "unopened-session.bkr",
        "--device",
        "42",
        "--seconds",
        "2",
        "--rate",
        "48000",
        "--channels",
        "2",
    ]
    .into_iter()
    .chain(extra.iter().copied())
    .map(str::to_owned)
    .collect()
}

fn append(arguments: &mut Vec<String>, extra: &[&str]) {
    arguments.extend(extra.iter().map(|value| (*value).to_owned()));
}

fn set(arguments: &mut [String], flag: &str, value: &str) {
    let index = arguments.iter().position(|item| item == flag).unwrap();
    arguments[index + 1] = value.to_owned();
}

fn host_args(host: Backend) -> Vec<String> {
    match host {
        Backend::Linux => args(&["--buffer-frames", "256", "--period-frames", "64"]),
        Backend::Macos => args(&["--buffer-frames", "256"]),
        Backend::Windows => args(&[]),
        _ => panic!("fixture requires a supported host"),
    }
}

fn asio_args() -> Vec<String> {
    let mut arguments = args(&[
        "--backend",
        "asio",
        "--asio-view",
        "native",
        "--output-channels",
        "0,1",
        "--asio-system-clock",
        "multimedia",
        "--asio-timer-error-ns",
        "0",
        "--asio-drift-error-ns",
        "0",
        "--asio-latency-error-ns",
        "0",
    ]);
    set(
        &mut arguments,
        "--device",
        "{12345678-9abc-def0-1234-56789abcdef0}",
    );
    arguments
}

fn encodings() -> [(&'static str, OutputEncoding, SampleEncoding, u16); 4] {
    [
        (
            "float32",
            OutputEncoding::Float32,
            SampleEncoding::Float32,
            4,
        ),
        (
            "pcm16",
            OutputEncoding::Pcm16,
            SampleEncoding::Pcm {
                container_bits: 16,
                valid_bits: 16,
            },
            2,
        ),
        (
            "pcm24",
            OutputEncoding::Pcm24,
            SampleEncoding::Pcm {
                container_bits: 24,
                valid_bits: 24,
            },
            3,
        ),
        (
            "pcm32",
            OutputEncoding::Pcm32,
            SampleEncoding::Pcm {
                container_bits: 32,
                valid_bits: 32,
            },
            4,
        ),
    ]
}

#[test]
fn output_encoding_omission_preserves_portable_float32_and_existing_defaults() {
    for host in [Backend::Windows, Backend::Linux, Backend::Macos] {
        let options = parse(&host_args(host), host).unwrap();
        assert_eq!(options.backend, host);
        assert_eq!(options.encoding, OutputEncoding::Float32);
        assert_eq!(options.format, AudioFormat::new(48_000, 2).unwrap());
        assert_eq!(options.speed.rate(), Rate::NORMAL);
        assert_eq!(options.output_rate, None);
        assert_eq!(options.seconds, Some(2));
        assert_eq!(options.channel_policy, ChannelPolicy::Exact);
        assert_eq!(options.preroll.as_nanos(), 3_000_000_000);
        assert_eq!(options.lookahead.as_nanos(), 3_000_000_000);
    }
    let options = parse(&asio_args(), Backend::Windows).unwrap();
    assert_eq!(options.backend, Backend::Asio);
    assert_eq!(options.encoding, OutputEncoding::Float32);
    assert_eq!(options.format, AudioFormat::new(48_000, 2).unwrap());
    assert_eq!(options.speed.rate(), Rate::NORMAL);
}

#[test]
fn output_encoding_four_exact_tokens_parse_for_implicit_and_explicit_alsa() {
    for (token, expected, _, _) in encodings() {
        for explicit_backend in [false, true] {
            let mut arguments = host_args(Backend::Linux);
            append(&mut arguments, &["--encoding", token]);
            if explicit_backend {
                append(&mut arguments, &["--backend", "alsa"]);
            }
            let options = parse(&arguments, Backend::Linux).unwrap();
            assert_eq!(options.backend, Backend::Linux);
            assert_eq!(options.encoding, expected, "{token}");
            assert_eq!(options.format, AudioFormat::new(48_000, 2).unwrap());
            assert_eq!(options.speed.rate(), Rate::NORMAL);
            assert_eq!(options.output_rate, None);
        }
    }
}

#[test]
fn output_encoding_explicit_selection_refuses_every_other_backend_including_float32() {
    let mut wasapi = host_args(Backend::Windows);
    append(&mut wasapi, &["--backend", "wasapi"]);
    let mut coreaudio = host_args(Backend::Macos);
    append(&mut coreaudio, &["--backend", "coreaudio"]);
    for (host, arguments, backend) in [
        (Backend::Windows, wasapi, Backend::Windows),
        (Backend::Macos, coreaudio, Backend::Macos),
        (Backend::Windows, asio_args(), Backend::Asio),
    ] {
        assert_eq!(parse(&arguments, host).unwrap().backend, backend);
        for (token, _, _, _) in encodings() {
            let mut explicit = arguments.clone();
            append(&mut explicit, &["--encoding", token]);
            let error = parse(&explicit, host).unwrap_err().to_string();
            assert!(error.contains("encoding"), "{backend:?}/{token}: {error}");
        }
    }
}

#[test]
fn output_encoding_blank_unknown_case_whitespace_duplicate_and_missing_values_refuse() {
    let base = host_args(Backend::Linux);
    assert!(parse(&base, Backend::Linux).is_ok());
    for token in [
        "",
        " ",
        "\t",
        "float64",
        "pcm8",
        "pcm24le",
        "Float32",
        "PCM16",
        "Pcm24",
        "pcm32 ",
        " pcm16",
        "float32\n",
        "pcm 24",
        "\0",
    ] {
        let mut invalid = base.clone();
        append(&mut invalid, &["--encoding", token]);
        let error = parse(&invalid, Backend::Linux).unwrap_err().to_string();
        assert!(error.contains("encoding"), "{token:?}: {error}");
    }
    for second in ["pcm16", "float32"] {
        let mut duplicate = base.clone();
        append(
            &mut duplicate,
            &["--encoding", "pcm16", "--encoding", second],
        );
        assert_eq!(
            parse(&duplicate, Backend::Linux).unwrap_err().to_string(),
            "duplicate option --encoding"
        );
    }
    let mut missing = base;
    missing.push("--encoding".to_owned());
    assert_eq!(
        parse(&missing, Backend::Linux).unwrap_err().to_string(),
        "each option requires a value"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn output_encoding_shipped_entry_refuses_malformed_values_before_file_or_device_io() {
    let base = host_args(Backend::Linux);
    validate_args(&base).unwrap();
    for extra in [
        vec!["--encoding", ""],
        vec!["--encoding", "PCM16"],
        vec!["--encoding", "pcm24 "],
        vec!["--encoding", "pcm16", "--encoding", "pcm32"],
        vec!["--encoding"],
    ] {
        let mut invalid = base.clone();
        append(&mut invalid, &extra);
        let parsed = parse(&invalid, Backend::Linux).unwrap_err().to_string();
        assert_eq!(run_args(&invalid).unwrap_err().to_string(), parsed);
    }
}

#[cfg(target_os = "linux")]
mod linux_requests {
    use super::*;
    use beatkernel::audio::{ChannelMatrix, ResampleQuality};
    use beatkernel::replay::codec::encode_replay;
    use beatkernel_bms_runtime::{
        replay_playback::{decode_section_setup, reconstruct_section},
        replay_speed_policy::wrap_header,
    };
    use beatkernel_platform::{audio::ConvertedNativeOutputState, linux::AlsaRequest};

    fn prepared() -> beatkernel_bms_runtime::PreparedBms {
        capture::prepared(
            capture::MAIN,
            48_000,
            &[
                (1, &[0.5, 0.25, -0.5, -0.25]),
                (2, &[0.75, 0.375, -0.75, -0.375]),
                (3, &[0.125, 0.0625, -0.125, -0.0625]),
            ],
        )
    }

    #[test]
    fn output_encoding_real_cold_request_has_exact_native_format_packing_sizing_and_domain() {
        for (token, choice, encoding, sample_bytes) in encodings() {
            for channels in [1u16, 2] {
                for output_rate in [None, Some(44_100u32)] {
                    let mut arguments = host_args(Backend::Linux);
                    set(&mut arguments, "--channels", &channels.to_string());
                    append(
                        &mut arguments,
                        &[
                            "--encoding",
                            token,
                            "--speed",
                            "0006/0004",
                            "--channel-policy",
                            "mono-stereo",
                            "--preroll-ns",
                            "7",
                            "--lookahead-ns",
                            "123456",
                        ],
                    );
                    if let Some(hz) = output_rate {
                        append(&mut arguments, &["--output-rate", &hz.to_string()]);
                    }
                    let options = parse(&arguments, Backend::Linux).unwrap();
                    let request = native::output_request(&options).unwrap();
                    let native_rate = output_rate.unwrap_or(48_000);
                    assert_eq!(
                        request,
                        AlsaRequest {
                            device: "42".to_owned(),
                            format: DeviceFormat::new(native_rate, channels, encoding, None)
                                .unwrap(),
                            buffer_frames: 256,
                            period_frames: 64,
                            allow_size_rounding: false,
                            monotonic_domain: HOST,
                        },
                        "{token}/{channels}/{output_rate:?}"
                    );
                    assert_eq!(request.format.block_align(), channels * sample_bytes);
                    assert_eq!(
                        request.format.bytes_per_second(),
                        native_rate * u32::from(channels * sample_bytes)
                    );
                    assert_eq!(options.encoding, choice);
                    assert_eq!(options.format, AudioFormat::new(48_000, channels).unwrap());
                    assert_eq!(options.output_rate, output_rate);
                    assert_eq!(options.speed.rate(), Rate::new(3, 2).unwrap());
                    assert_eq!(options.channel_policy, ChannelPolicy::MonoToStereo);
                    assert_eq!(options.preroll.as_nanos(), 7);
                    assert_eq!(options.lookahead.as_nanos(), 123456);
                    assert_eq!(options.seconds, Some(2));
                }
            }
        }
        let options = parse(&host_args(Backend::Linux), Backend::Linux).unwrap();
        let request = native::output_request(&options).unwrap();
        assert_eq!(
            request.format,
            DeviceFormat::new(48_000, 2, SampleEncoding::Float32, None).unwrap()
        );
        assert_eq!(options.format, AudioFormat::new(48_000, 2).unwrap());
    }

    #[test]
    fn output_encoding_preserves_complete_recording_conditions_judgments_and_source_mixer() {
        let ts = Timestamp::from_nanos;
        let prepared = prepared();
        let mut recording = capture::captured_at(
            &prepared,
            0,
            ts(500_000_000),
            Some(ts(4_000_000_001)),
            Some(4_000_000_001),
        );
        let source_speed: ReplaySpeed = "3/2".parse().unwrap();
        recording.header = wrap_header(recording.header, source_speed, capture::limits()).unwrap();
        recording.calibration_metadata = Some(vec![0x11, 0x22, 0x33]);
        let original = recording.clone();
        let canonical = encode_replay(&recording, capture::limits()).unwrap();
        let decoded =
            read_replay(&mut std::io::Cursor::new(&canonical), capture::limits()).unwrap();
        assert_eq!(decoded, original);
        let conditions = decode_section_setup(&decoded.header.options).unwrap();
        assert_eq!(conditions.start, ts(500_000_000));
        assert_eq!(conditions.end, Some(ts(4_000_000_001)));
        assert_eq!(conditions.source_speed, source_speed);
        let reconstructed =
            reconstruct_section(&prepared.source, decoded.clone(), capture::limits()).unwrap();
        assert_eq!(reconstructed.header(), &original.header);
        assert_eq!(reconstructed.records(), original.records.as_slice());
        let origin = ClockPoint {
            domain: OUTPUT,
            timestamp: Timestamp::ZERO,
        };
        let mut reference = None;
        for (token, _, encoding, _) in encodings() {
            let playback_prepared = self::prepared();
            let mut arguments = host_args(Backend::Linux);
            set(&mut arguments, "--channels", "1");
            append(
                &mut arguments,
                &[
                    "--encoding",
                    token,
                    "--output-rate",
                    "44100",
                    "--speed",
                    "2/1",
                    "--preroll-ns",
                    "1000000",
                ],
            );
            let options = parse(&arguments, Backend::Linux).unwrap();
            let request = native::output_request(&options).unwrap();
            let plan = plan_section_audio_with_rate(
                &playback_prepared,
                decoded.clone(),
                capture::limits(),
                origin,
                options.preroll,
                options.speed.rate(),
            )
            .unwrap();
            assert_eq!(plan.judge_events, reconstructed.results());
            assert_eq!(
                plan.final_judge_hash,
                reconstructed.engine().stable_hash().unwrap()
            );
            // The source grid remains 48 kHz: (3.500000001 s / 2, truncated
            // to whole ns, + 0.001 s preroll) occupies exactly 84,048 frames.
            assert_eq!(plan.playback_end_frame, Some(84_048));
            let summary = (
                plan.commands.clone(),
                plan.judge_events.clone(),
                plan.recorded_until,
                plan.final_judge_hash,
                plan.playback_end_frame,
            );
            if let Some(expected) = &reference {
                assert_eq!(&summary, expected);
            } else {
                reference = Some(summary);
            }
            let (mut producer, consumer) = command_queue(16).unwrap();
            for command in plan.commands {
                producer.try_push(command).unwrap();
            }
            let mixer = Mixer::new(
                MixerConfig::new(
                    options.format,
                    OUTPUT,
                    Timestamp::ZERO,
                    AudioLimits::new(16, 4, 16, 256, 16).unwrap(),
                )
                .with_playback_end_frame(plan.playback_end_frame.unwrap()),
                playback_prepared.bank,
                consumer,
            )
            .unwrap();
            let mut converted = ConvertedNativeOutputState::new(
                mixer,
                request.format,
                ChannelMatrix::default_mix(1, 1).unwrap(),
                ResampleQuality::Linear,
                request.period_frames as usize,
            )
            .unwrap_or_else(|_| panic!("actual replay converter setup"));
            assert_eq!(converted.format().encoding(), encoding);
            assert_eq!(converted.format().sample_rate(), 44_100);
            assert_eq!(converted.mixer().config().format(), options.format);
            let report = converted.render_pending(64).unwrap();
            assert_eq!(report.source_rate, 48_000);
            assert_eq!(report.target_rate, 44_100);
            assert_eq!(recording, original, "complete capture model: {token}");
            assert_eq!(
                encode_replay(&recording, capture::limits()).unwrap(),
                canonical
            );
            assert_eq!(
                decode_section_setup(&recording.header.options).unwrap(),
                conditions
            );
        }
    }
}
