//! Production CLI and cold requests; no endpoint, input device or native timing is fabricated.
use super::{parse, validate_args};
use beatkernel_bms_runtime::settings::OutputEncoding;

const CHOICES: [(&str, OutputEncoding); 4] = [
    ("float32", OutputEncoding::Float32),
    ("pcm16", OutputEncoding::Pcm16),
    ("pcm24", OutputEncoding::Pcm24),
    ("pcm32", OutputEncoding::Pcm32),
];

// Keep every required argument valid, so encoding failures are not setup failures.
fn args(roster: usize) -> Vec<String> {
    let mut supplied: Vec<String> = [
        "--chart",
        "fixture.bms",
        "--alsa",
        "hw:1,0",
        "--rate",
        "44100",
        "--channels",
        "2",
        "--period-frames",
        "256",
        "--buffer-frames",
        "1024",
        "--bind",
        "11:04",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    supplied.extend(
        match roster {
            0 => vec!["--evdev", "/dev/input/event4"],
            1 => vec![
                "--local-input",
                "/dev/input/event4",
                "--local-input",
                "/dev/input/event5",
                "--local-input",
                "/dev/input/event6",
            ],
            2 => vec![
                "--local-player",
                "7:/dev/input/by-id/keyboard:a",
                "--local-player",
                "1000:/dev/input/event5",
            ],
            _ => unreachable!(),
        }
        .into_iter()
        .map(str::to_owned),
    );
    supplied
}

fn with_encoding(base: &[String], token: &str) -> Vec<String> {
    let mut supplied = base.to_vec();
    supplied.extend(["--encoding".into(), token.into()]);
    supplied
}

#[test]
fn output_encoding_defaults_to_float32_for_solo_and_both_local_rosters() {
    for roster in 0..3 {
        let supplied = args(roster);
        assert!(validate_args(&supplied).is_ok());
        let options = parse(&supplied).unwrap();
        assert_eq!(options.encoding, OutputEncoding::Float32);
        assert_eq!(options.format.sample_rate(), 44_100);
        assert_eq!(options.format.channels(), 2);
        assert_eq!(options.local_inputs.len(), [0, 3, 2][roster]);
    }
}

#[test]
fn output_encoding_accepts_each_exact_choice_without_changing_input_assignments() {
    for roster in 0..3 {
        let base = args(roster);
        let original = parse(&base).unwrap();
        for (token, choice) in CHOICES {
            let supplied = with_encoding(&base, token);
            assert!(validate_args(&supplied).is_ok(), "roster={roster} {token}");
            let options = parse(&supplied).unwrap();
            assert_eq!(options.encoding, choice);
            assert_eq!(options.evdev, original.evdev);
            assert_eq!(options.local_inputs, original.local_inputs);
            assert_eq!(options.local_players, original.local_players);
            assert_eq!(options.bindings, original.bindings);
            assert_eq!(options.format, original.format);
        }
    }
}

#[test]
fn output_encoding_refuses_empty_unknown_whitespace_and_case_changes() {
    for roster in 0..3 {
        let base = args(roster);
        assert!(validate_args(&base).is_ok());
        for invalid in [
            "",
            " ",
            "\t",
            "pcm8",
            "pcm20",
            "pcm24-in-32",
            "float64",
            "Float32",
            "PCM16",
            "Pcm24",
            "PCM32",
            " float32",
            "pcm16 ",
            "\tpcm24",
            "pcm32\n",
        ] {
            let supplied = with_encoding(&base, invalid);
            let error = match parse(&supplied) {
                Ok(_) => panic!("roster={roster} accepted encoding {invalid:?}"),
                Err(error) => error.to_string(),
            };
            assert!(error.contains("encoding"), "wrong refusal: {error}");
            assert!(validate_args(&supplied).is_err());
        }
    }
}

#[test]
fn output_encoding_refuses_duplicate_choices_and_missing_value() {
    for roster in 0..3 {
        let base = args(roster);
        for first in ["float32", "pcm16"] {
            for second in [first, "pcm24"] {
                let mut supplied = with_encoding(&base, first);
                supplied.extend(["--encoding".into(), second.into()]);
                let error = match parse(&supplied) {
                    Ok(_) => panic!("duplicate encoding was accepted"),
                    Err(error) => error.to_string(),
                };
                assert!(error.contains("duplicate option --encoding"), "{error}");
                assert!(validate_args(&supplied).is_err());
            }
        }
        let mut missing = base;
        missing.push("--encoding".into());
        let error = match parse(&missing) {
            Ok(_) => panic!("missing encoding value was accepted"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("every option requires a value"), "{error}");
        assert!(validate_args(&missing).is_err());
    }
}

#[cfg(target_os = "linux")]
mod native_requests {
    use super::*;
    use super::super::{native, Options};
    use std::path::PathBuf;
    use beatkernel::{
        audio::{
            AudioCommand, PcmLimits, PcmSample, PracticeLimits, PracticeRegion, SampleBank,
            SampleId, VoiceId,
        },
        time::{Duration, Timestamp},
    };
    use beatkernel_bms_runtime::{
        native_audio::{
            prepare_audio_with_speed, prepare_retained_audio_with_speed, NativeAudioConfig,
            NativePracticeAudioConfig,
        },
        native_judge::{
            capture_limits, prepare_section_capture_for_policy_with_speed, NativeJudgeConfig,
        },
        play_policy::ResolvedPlayPolicy,
        replay_playback::decode_section_setup,
    };
    use beatkernel_platform::audio::{encode_pcm, SampleEncoding};

    const NATIVE_CHOICES: [(SampleEncoding, u16); 4] = [
        (SampleEncoding::Float32, 4),
        (
            SampleEncoding::Pcm {
                container_bits: 16,
                valid_bits: 16,
            },
            2,
        ),
        (
            SampleEncoding::Pcm {
                container_bits: 24,
                valid_bits: 24,
            },
            3,
        ),
        (
            SampleEncoding::Pcm {
                container_bits: 32,
                valid_bits: 32,
            },
            4,
        ),
    ];

    #[test]
    fn output_encoding_cold_request_carries_exact_format_and_native_configuration() {
        for roster in 0..3 {
            for channels in [1, 2] {
                let mut base = args(roster);
                let index = base.iter().position(|value| value == "--channels").unwrap();
                base[index + 1] = channels.to_string();
                for token in [
                    None,
                    Some("float32"),
                    Some("pcm16"),
                    Some("pcm24"),
                    Some("pcm32"),
                ] {
                    let supplied =
                        token.map_or_else(|| base.clone(), |token| with_encoding(&base, token));
                    let options = parse(&supplied).unwrap();
                    let index = CHOICES
                        .iter()
                        .position(|(_, choice)| *choice == options.encoding)
                        .unwrap();
                    let (encoding, width) = NATIVE_CHOICES[index];
                    let request = native::output_request(&options).unwrap();
                    assert_eq!(request.device, "hw:1,0");
                    assert_eq!(request.format.encoding(), encoding);
                    assert_eq!(request.format.encoding().bytes_per_sample(), width);
                    assert_eq!(request.format.pcm(), options.format);
                    assert_eq!(request.format.sample_rate(), 44_100);
                    assert_eq!(request.format.channels(), channels);
                    assert_eq!(request.format.channel_mask(), None);
                    assert_eq!(request.format.block_align(), channels * width);
                    assert_eq!(
                        request.format.bytes_per_second(),
                        44_100 * u32::from(channels * width)
                    );
                    assert_eq!((request.period_frames, request.buffer_frames), (256, 1024));
                    assert!(!request.allow_size_rounding);
                    assert_eq!(request.monotonic_domain, native::HOST);
                    assert_eq!(options.format.sample_rate(), 44_100);
                    assert_eq!(options.alsa, "hw:1,0");
                }
            }
        }
    }

    #[test]
    fn output_encoding_cold_request_drives_real_encoder_signed_bounds_mono_and_stereo() {
        let samples = [-1.0, -0.5, 0.0, 0.5, 1.0];
        let expected: [&[u8]; 4] = [
            &[
                0, 0, 128, 191, 0, 0, 0, 191, 0, 0, 0, 0, 0, 0, 0, 63, 0, 0, 128, 63,
            ],
            &[0, 128, 0, 192, 0, 0, 0, 64, 255, 127],
            &[0, 0, 128, 0, 0, 192, 0, 0, 0, 0, 0, 64, 255, 255, 127],
            &[
                0, 0, 0, 128, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 0, 64, 255, 255, 255, 127,
            ],
        ];
        for channels in [1u16, 2] {
            let mut base = args(0);
            let index = base.iter().position(|value| value == "--channels").unwrap();
            base[index + 1] = channels.to_string();
            for (choice_index, (token, _)) in CHOICES.into_iter().enumerate() {
                let options = parse(&with_encoding(&base, token)).unwrap();
                let request = native::output_request(&options).unwrap();
                let input: Vec<_> = samples
                    .iter()
                    .flat_map(|sample| std::iter::repeat_n(*sample, usize::from(channels)))
                    .collect();
                let width = usize::from(request.format.encoding().bytes_per_sample());
                let wanted: Vec<_> = expected[choice_index]
                    .chunks_exact(width)
                    .flat_map(|sample| {
                        std::iter::repeat_n(sample, usize::from(channels))
                            .flatten()
                            .copied()
                    })
                    .collect();
                let mut encoded = vec![0x55; input.len() * width];
                encode_pcm(request.format, &input, &mut encoded).unwrap();
                assert_eq!(encoded, wanted, "{token}, channels={channels}");
            }
        }
    }

    fn capture_bytes(options: &Options) -> Vec<u8> {
        let source =
            beatkernel_bms::parse("#BPM 120\n#WAV01 note.wav\n#00011:01\n", Default::default())
                .unwrap();
        let policy =
            ResolvedPlayPolicy::builtin(options.early, options.late, options.offset).unwrap();
        let config = NativeJudgeConfig {
            early: options.early,
            late: options.late,
            offset: options.offset,
            preroll: options.preroll,
            output: native::OUTPUT,
            end: options.end_ns.map(Timestamp::from_nanos),
        };
        let judge = config
            .judge_with_policy(&source, source.compile().unwrap().chart, &policy)
            .unwrap();
        let capture = prepare_section_capture_for_policy_with_speed(
            &source,
            &judge,
            &policy,
            native::LOGICAL,
            options.live_speed.start(),
            options.chart_seed,
            options.live_speed.end(),
            capture_limits(
                options.record_replay.is_some(),
                options.replay_max_bytes,
                options.replay_max_records,
            )
            .unwrap(),
            options.live_speed.speed(),
        )
        .unwrap()
        .unwrap();
        let setup = decode_section_setup(&capture.header().options).unwrap();
        assert_eq!(setup.source_speed, options.live_speed.speed());
        assert_eq!(setup.start, options.live_speed.start());
        assert_eq!(setup.end, options.live_speed.end());
        assert_eq!(setup.chart_seed, options.chart_seed);
        assert_eq!(capture.header().normalized_clock, native::LOGICAL);
        capture.into_bytes().unwrap()
    }

    fn source_audio(
        options: &Options,
        retained: bool,
    ) -> (Vec<f32>, beatkernel::audio::AudioCounters) {
        let limits = PcmLimits::new(4096, 4096, 1).unwrap();
        let mut bank = SampleBank::new(options.format, limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(
                options.format,
                vec![0.25; 128 * usize::from(options.format.channels())],
                limits,
            )
            .unwrap(),
        )
        .unwrap();
        let commands = vec![AudioCommand::Play {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: options.live_speed.start(),
            gain: 1.0,
        }];
        let config = NativeAudioConfig {
            output_origin: native::output_origin(),
            start: options.live_speed.start(),
            preroll: Duration::from_nanos(options.preroll),
            lookahead: Duration::from_nanos(options.bgm_lookahead),
            voices: 2,
            max_render_frames: 64,
            playback_end_frame: if retained {
                None
            } else {
                options.playback_end().unwrap()
            },
            gated_start: false,
        };
        let mut audio = if retained {
            prepare_retained_audio_with_speed(
                bank,
                commands,
                config,
                NativePracticeAudioConfig {
                    region: PracticeRegion::new(
                        options.live_speed.start(),
                        options.live_speed.end().unwrap(),
                        false,
                    )
                    .unwrap(),
                    limits: PracticeLimits::new(1, 1, 64, 8, 16).unwrap(),
                },
                options.live_speed.speed(),
                options.live_speed.end(),
            )
            .unwrap()
            .audio
        } else {
            prepare_audio_with_speed(bank, commands, config, options.live_speed.speed()).unwrap()
        };
        assert_eq!(audio.mixer.config().format(), options.format);
        assert_eq!(audio.bgm.config().sample_rate, options.format.sample_rate());
        let mut pcm = vec![0.0; 64 * usize::from(options.format.channels())];
        audio.mixer.render(&mut pcm).unwrap();
        assert!(
            pcm.iter().any(|sample| *sample != 0.0),
            "real source audio required"
        );
        assert_eq!(audio.mixer.config().format().sample_rate(), 44_100);
        assert_eq!(audio.mixer.rate(), options.live_speed.speed().rate());
        (pcm, audio.mixer.counters())
    }

    #[test]
    fn output_encoding_preserves_selected_speed_retained_source_and_recording_bytes() {
        for roster in 0..3 {
            for fraction in ["1/1", "2/1", "3/2", "4/3", "1/2"] {
                let mut base = args(roster);
                base.extend([
                    "--speed".into(),
                    fraction.into(),
                    "--start-ns".into(),
                    "1000000000".into(),
                    "--end-ns".into(),
                    "1010000000".into(),
                    "--preroll-ns".into(),
                    "7".into(),
                    "--record-replay".into(),
                    "capture.bkr".into(),
                    "--chart-seed".into(),
                    "123".into(),
                    "--replay-max-records".into(),
                    "8".into(),
                    "--replay-max-bytes".into(),
                    "65536".into(),
                ]);
                let original = parse(&base).unwrap();
                let recorded = capture_bytes(&original);
                let ordinary_audio = source_audio(&original, false);
                let retained_audio = source_audio(&original, true);
                for (token, choice) in CHOICES {
                    let supplied = with_encoding(&base, token);
                    assert!(validate_args(&supplied).is_ok());
                    let options = parse(&supplied).unwrap();
                    let request = native::output_request(&options).unwrap();
                    assert_eq!(options.encoding, choice);
                    assert_eq!(request.format.pcm(), original.format);
                    assert_eq!(options.live_speed, original.live_speed);
                    assert_eq!(
                        options.playback_end().unwrap(),
                        original.playback_end().unwrap()
                    );
                    assert_eq!(
                        options.song_origin().unwrap(),
                        original.song_origin().unwrap()
                    );
                    assert_eq!(options.selected_speed(), fraction != "1/1");
                    assert_eq!(options.record_replay, Some(PathBuf::from("capture.bkr")));
                    assert_eq!(
                        (options.replay_max_records, options.replay_max_bytes),
                        (8, 65536)
                    );
                    assert_eq!(
                        capture_bytes(&options),
                        recorded,
                        "{fraction} {token}: recording changed"
                    );
                    assert_eq!(
                        source_audio(&options, false),
                        ordinary_audio,
                        "{fraction} {token}: ordinary source changed"
                    );
                    assert_eq!(
                        source_audio(&options, true),
                        retained_audio,
                        "{fraction} {token}: retained source changed"
                    );
                }
            }
        }
    }
}
