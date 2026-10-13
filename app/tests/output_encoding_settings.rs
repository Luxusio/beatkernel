//! Exact output selection through the bounded draft, profile and Watch projection.
use beatkernel_bms_runtime::{
    gameplay::output::adapters::encoding::{encoding_choice, native_encoding},
    native_defaults::{
        complete, prepare, replay_args, NativeDefaults, NativeKeyboardCandidate,
        NativeOutputFormat, NativePreparationDevice, NativePreparationResult, PreparationMode,
    },
    presentation_settings::PresentationSettings,
    settings::{
        overlay_native_args, NativeSettings, OutputEncoding, SettingsHost, MAX_FIELDS,
        MAX_TOTAL_BYTES, MAX_VALUE_BYTES,
    },
    settings_profile::{
        decode_player_profile, decode_profile, encode_player_profile, encode_profile,
        PlayerProfile, MAX_PROFILE_BYTES,
    },
};
use beatkernel_platform::audio::{encode_pcm, DeviceFormat, SampleEncoding};

const TOKENS: [&str; 4] = [
    OutputEncoding::Float32.as_str(),
    OutputEncoding::Pcm16.as_str(),
    OutputEncoding::Pcm24.as_str(),
    OutputEncoding::Pcm32.as_str(),
];
const HOST: SettingsHost = SettingsHost::Linux;

fn args(tokens: &[&str]) -> Vec<String> {
    tokens.iter().map(|token| (*token).to_owned()).collect()
}

fn value<'a>(tokens: &'a [String], flag: &str) -> Option<&'a str> {
    tokens
        .chunks_exact(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn encoding_row(model: &NativeSettings) -> usize {
    model
        .fields()
        .iter()
        .position(|row| row.flag == "--encoding")
        .unwrap()
}

#[test]
fn exact_tokens_const_display_and_default_are_strict() {
    assert_eq!(TOKENS, ["float32", "pcm16", "pcm24", "pcm32"]);
    assert_eq!(OutputEncoding::default().as_str(), "float32");
    for token in TOKENS {
        let parsed: Result<OutputEncoding, String> = token.parse();
        let choice = parsed.unwrap();
        assert_eq!(choice.as_str(), token);
        assert_eq!(
            encoding_choice(native_encoding(choice)).unwrap().as_str(),
            token
        );
    }
    for invalid in [
        "",
        "pcm",
        "pcm8",
        "pcm20",
        "pcm64",
        "float",
        "float64",
        "PCM16",
        "Float32",
        " float32",
        "pcm24 ",
        "pcm32\n",
        "pcm16\t",
        "pcm\u{2028}16",
        "ｐｃｍ１６",
    ] {
        assert!(
            invalid.parse::<OutputEncoding>().is_err(),
            "accepted {invalid:?}"
        );
    }
}

#[test]
fn selected_native_format_runs_real_encoder_at_signed_bounds_without_changing_pcm_grid() {
    let samples = [-1.0, -0.5, 0.0, 0.5, 1.0, -2.0, 2.0, 0.0];
    let before = samples;
    let expected_pcm: [&[u8]; 3] = [
        &[
            0x00, 0x80, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x40, 0xff, 0x7f, 0x00, 0x80, 0xff, 0x7f,
            0x00, 0x00,
        ],
        &[
            0x00, 0x00, 0x80, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0xff, 0xff,
            0x7f, 0x00, 0x00, 0x80, 0xff, 0xff, 0x7f, 0x00, 0x00, 0x00,
        ],
        &[
            0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x40, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x00, 0x00, 0x80, 0xff, 0xff, 0xff, 0x7f,
            0x00, 0x00, 0x00, 0x00,
        ],
    ];
    for channels in [1, 2] {
        for (token, width) in [("float32", 4), ("pcm16", 2), ("pcm24", 3), ("pcm32", 4)] {
            let format = DeviceFormat::new(
                44_100,
                channels,
                native_encoding(token.parse().unwrap()),
                if channels == 2 { Some(3) } else { None },
            )
            .unwrap();
            assert_eq!(format.pcm().sample_rate(), 44_100);
            assert_eq!(format.pcm().channels(), channels);
            assert_eq!(format.block_align(), channels * width);
            assert_eq!(
                format.bytes_per_second(),
                44_100 * u32::from(channels * width)
            );
            assert_eq!(encoding_choice(format.encoding()).unwrap().as_str(), token);
            let mut bytes = vec![0xa5; samples.len() * usize::from(width)];
            encode_pcm(format, &samples, &mut bytes).unwrap();
            match token {
                "float32" => {
                    let expected: Vec<u8> = samples
                        .iter()
                        .flat_map(|sample| sample.to_le_bytes())
                        .collect();
                    assert_eq!(bytes, expected);
                }
                "pcm16" => assert_eq!(bytes, expected_pcm[0]),
                "pcm24" => assert_eq!(bytes, expected_pcm[1]),
                "pcm32" => assert_eq!(bytes, expected_pcm[2]),
                _ => unreachable!(),
            }
            assert_eq!(samples, before);
        }
    }
}

#[test]
fn unsupported_container_and_valid_bit_pairs_never_get_a_closest_display_token() {
    for container_bits in [0, 8, 16, 24, 32, 64, u16::MAX] {
        for valid_bits in (0..=33).chain([u16::MAX]) {
            let encoding = SampleEncoding::Pcm {
                container_bits,
                valid_bits,
            };
            if matches!(container_bits, 16 | 24 | 32) && valid_bits == container_bits {
                let token = format!("pcm{container_bits}");
                assert_eq!(encoding_choice(encoding).unwrap().as_str(), token);
            } else {
                assert!(encoding_choice(encoding).is_err(), "accepted {encoding:?}");
            }
        }
    }
    // These are valid native formats, but this exact Linux UI selection cannot express them.
    for (container_bits, valid_bits) in [(16, 12), (24, 20), (32, 24)] {
        let format = DeviceFormat::new(
            48_000,
            2,
            SampleEncoding::Pcm {
                container_bits,
                valid_bits,
            },
            Some(3),
        )
        .unwrap();
        assert!(encoding_choice(format.encoding()).is_err());
    }
}

#[test]
fn linux_encoding_appends_after_the_entire_old_schema_and_foreign_hosts_stay_exact() {
    let common = [
        "--timing-preset",
        "--rank-precedence",
        "--gauge",
        "--chart-seed",
        "--start-ns",
        "--end-ns",
        "--bind",
        "--early-ns",
        "--late-ns",
        "--input-offset-ns",
        "--preroll-ns",
        "--bgm-lookahead-ns",
        "--voices",
        "--channel-policy",
        "--seconds",
        "--record-replay",
        "--replay-max-records",
        "--replay-max-bytes",
        "--ghost-self",
        "--ghost-other",
        "--mp-host",
        "--mp-join",
        "--mp-webtransport",
        "--mp-room",
        "--mp-role",
        "--mp-origin",
        "--mp-timeout-ms",
        "--mp-cert",
        "--mp-key",
        "--mp-ca",
        "--mp-server-name",
        "--speed",
    ];
    for (host, platform) in [
        (
            SettingsHost::Windows,
            &[
                "--local-player",
                "--advance-lag-ns",
                "--keyboard-path",
                "--device",
                "--backend",
                "--mode",
                "--buffer",
                "--period",
                "--shared-policy",
                "--asio-view",
                "--output-channels",
                "--asio-system-clock",
                "--asio-timer-error-ns",
                "--asio-drift-error-ns",
                "--asio-latency-error-ns",
                "--asio-anchor-age-ns",
            ][..],
        ),
        (
            HOST,
            &[
                "--local-player",
                "--local-input",
                "--evdev",
                "--alsa",
                "--rate",
                "--channels",
                "--period-frames",
                "--buffer-frames",
                "--advance-lag-ns",
            ][..],
        ),
        (
            SettingsHost::Macos,
            &[
                "--local-player",
                "--device",
                "--keyboard-registry",
                "--rate",
                "--channels",
                "--buffer-frames",
                "--advance-lag-ns",
            ][..],
        ),
    ] {
        let model = NativeSettings::from_args(&[], host).unwrap();
        let mut expected = platform.to_vec();
        expected.extend(common);
        if host == HOST {
            expected.push("--encoding");
        }
        assert_eq!(
            model
                .fields()
                .iter()
                .map(|row| row.flag)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(model.native_args().is_empty());
    }
}

#[test]
fn initial_live_and_applied_linux_fields_retain_exact_tokens_and_empty_live_preserves() {
    for token in TOKENS {
        let supplied = args(&["--encoding", token]);
        for model in [
            NativeSettings::from_args(&supplied, HOST).unwrap(),
            NativeSettings::output_only(&supplied, HOST).unwrap(),
            NativeSettings::output_capability(&supplied, HOST).unwrap(),
        ] {
            assert_eq!(model.native_args(), supplied);
            let row = &model.fields()[encoding_row(&model)];
            assert!(!row.label.is_empty());
            for supported in TOKENS {
                assert!(
                    row.hint.contains(supported),
                    "missing {supported} in {:?}",
                    row.hint
                );
            }
        }
    }
    let mut live = NativeSettings::output_only(&args(&["--encoding", "pcm24"]), HOST).unwrap();
    let row = encoding_row(&live);
    live.set_value(row, "").unwrap();
    assert_eq!(live.fields()[row].value, "");
    assert!(live.native_args().is_empty());
    assert!(
        NativeSettings::output_only(&args(&["--encoding", ""]), HOST)
            .unwrap()
            .native_args()
            .is_empty()
    );
    assert!(NativeSettings::output_capability(&[], HOST)
        .unwrap()
        .fields()
        .is_empty());
}

#[test]
fn duplicate_unpaired_and_foreign_encoding_fields_keep_schema_refusals() {
    for supplied in [
        args(&["--encoding", "pcm16", "--encoding", "pcm24"]),
        args(&["--encoding", "pcm16", "--encoding", ""]),
        args(&["--encoding"]),
        args(&["--future-encoding", "pcm16"]),
    ] {
        assert!(NativeSettings::from_args(&supplied, HOST).is_err());
        assert!(NativeSettings::output_only(&supplied, HOST).is_err());
        assert!(NativeSettings::output_capability(&supplied, HOST).is_err());
    }
    for host in [SettingsHost::Windows, SettingsHost::Macos] {
        for token in TOKENS.into_iter().chain([""]) {
            let supplied = args(&["--encoding", token]);
            assert!(NativeSettings::from_args(&supplied, host).is_err());
            assert!(NativeSettings::output_only(&supplied, host).is_err());
            assert!(NativeSettings::output_capability(&supplied, host).is_err());
        }
    }
}

#[test]
fn draft_value_field_count_and_total_byte_bounds_remain_atomic() {
    let mut model = NativeSettings::from_args(&args(&["--encoding", "pcm24"]), HOST).unwrap();
    let row = encoding_row(&model);
    let oversized = "x".repeat(MAX_VALUE_BYTES + 1);
    for invalid in [
        oversized.as_str(),
        "pcm16\t",
        "pcm24\n",
        "pcm32\u{2028}",
        "pcm32\u{2029}",
    ] {
        let before = model.native_args();
        assert!(model.set_value(row, invalid).is_err());
        assert_eq!(model.native_args(), before);
        assert!(NativeSettings::from_args(&args(&["--encoding", invalid]), HOST).is_err());
        assert!(NativeSettings::output_only(&args(&["--encoding", invalid]), HOST).is_err());
    }
    model.set_value(row, &"x".repeat(MAX_VALUE_BYTES)).unwrap();
    assert_eq!(model.fields()[row].value.len(), MAX_VALUE_BYTES);
    model.set_value(row, "pcm24").unwrap();
    let mut remaining = MAX_TOTAL_BYTES - "pcm24".len();
    for index in 0..model.fields().len() {
        if index == row || remaining == 0 {
            continue;
        }
        let length = remaining.min(MAX_VALUE_BYTES);
        model.set_value(index, &"x".repeat(length)).unwrap();
        remaining -= length;
    }
    assert_eq!(remaining, 0);
    assert_eq!(
        model
            .fields()
            .iter()
            .map(|field| field.value.len())
            .sum::<usize>(),
        MAX_TOTAL_BYTES
    );
    let before = model.native_args();
    assert!(model.set_value(row, "float32").is_err());
    assert_eq!(model.native_args(), before);
    let too_many: Vec<String> = (0..=MAX_FIELDS)
        .flat_map(|_| args(&["--bind", "11:04"]))
        .collect();
    assert!(NativeSettings::from_args(&too_many, HOST).is_err());
    let too_large: Vec<String> = (0..=(MAX_TOTAL_BYTES / MAX_VALUE_BYTES))
        .flat_map(|_| vec!["--bind".to_owned(), "x".repeat(MAX_VALUE_BYTES)])
        .collect();
    assert!(NativeSettings::from_args(&too_large, HOST).is_err());
}

#[test]
fn encoding_overlay_changes_only_its_flag_and_empty_initial_override_returns_to_default() {
    let base = args(&[
        "--encoding",
        "pcm16",
        "--rate",
        "44100",
        "--speed",
        "3/2",
        "--bind",
        "11:04",
        "--bind",
        "12:05",
    ]);
    for token in TOKENS {
        let merged = overlay_native_args(&base, &args(&["--encoding", token]), HOST).unwrap();
        let expected = args(&[
            "--rate",
            "44100",
            "--speed",
            "3/2",
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--encoding",
            token,
        ]);
        assert_eq!(merged, expected);
        assert_eq!(base[1], "pcm16");
    }
    assert_eq!(
        overlay_native_args(&base, &args(&["--encoding", ""]), HOST).unwrap(),
        args(&["--rate", "44100", "--speed", "3/2", "--bind", "11:04", "--bind", "12:05"]),
    );
    assert!(overlay_native_args(
        &base,
        &args(&["--encoding", "pcm16", "--encoding", "pcm24"]),
        HOST
    )
    .is_err());
}

#[test]
fn real_v1_and_v2_profiles_roundtrip_encoding_with_order_speed_seed_and_display_unchanged() {
    for token in TOKENS {
        let supplied = args(&[
            "--speed",
            "3/2",
            "--encoding",
            token,
            "--chart-seed",
            "18446744073709551615",
            "--bind",
            "11:04",
            "--bind",
            "12:05",
        ]);
        let model = NativeSettings::from_args(&supplied, HOST).unwrap();
        let encoded = encode_profile(&model, HOST).unwrap();
        assert_eq!(encoded, format!("BEATKERNEL-NATIVE-PROFILE\t1\tlinux\n--speed\t3/2\n--encoding\t{token}\n--chart-seed\t18446744073709551615\n--bind\t11:04\n--bind\t12:05\n").into_bytes());
        let restored = decode_profile(&encoded, HOST).unwrap();
        assert_eq!(restored.native_args(), supplied);
        assert_eq!(restored.chart_seed().unwrap(), u64::MAX);
        let speed = restored.playback_speed().unwrap().rate();
        assert_eq!((speed.numerator(), speed.denominator()), (3, 2));
        let upgraded = decode_player_profile(&encoded, HOST).unwrap();
        assert_eq!(upgraded.native.native_args(), supplied);
        assert_eq!(upgraded.presentation, PresentationSettings::default());
        let presentation = PresentationSettings::default()
            .apply_overrides(&args(&["--ui-fps", "144", "--ui-lookahead-ms", "500"]))
            .unwrap();
        let profile = PlayerProfile {
            native: model,
            presentation,
        };
        let encoded = encode_player_profile(&profile, HOST).unwrap();
        let mut expected = format!("BEATKERNEL-NATIVE-PROFILE\t2\tlinux\n--speed\t3/2\n--encoding\t{token}\n--chart-seed\t18446744073709551615\n--bind\t11:04\n--bind\t12:05\n");
        for pair in profile.presentation.args().chunks_exact(2) {
            expected.push_str(&format!("{}\t{}\n", pair[0], pair[1]));
        }
        assert_eq!(encoded, expected.into_bytes());
        let restored = decode_player_profile(&encoded, HOST).unwrap();
        assert_eq!(restored.native.native_args(), supplied);
        assert_eq!(restored.presentation, profile.presentation);
        assert!(decode_profile(&encoded, HOST).is_err());
    }
}

#[test]
fn omission_preserves_old_profile_bytes_and_native_float32_default() {
    let model = NativeSettings::from_args(&[], HOST).unwrap();
    assert_eq!(model.fields()[encoding_row(&model)].value, "");
    let old_v1 = b"BEATKERNEL-NATIVE-PROFILE\t1\tlinux\n";
    assert_eq!(encode_profile(&model, HOST).unwrap(), old_v1);
    assert!(decode_profile(old_v1, HOST)
        .unwrap()
        .native_args()
        .is_empty());
    let profile = PlayerProfile {
        native: model,
        presentation: PresentationSettings::default(),
    };
    let encoded = encode_player_profile(&profile, HOST).unwrap();
    let mut expected = "BEATKERNEL-NATIVE-PROFILE\t2\tlinux\n".to_owned();
    for pair in profile.presentation.args().chunks_exact(2) {
        expected.push_str(&format!("{}\t{}\n", pair[0], pair[1]));
    }
    assert_eq!(encoded, expected.into_bytes());
    assert!(decode_player_profile(&encoded, HOST)
        .unwrap()
        .native
        .native_args()
        .is_empty());
    assert_eq!(
        native_encoding(OutputEncoding::default()),
        SampleEncoding::Float32
    );
}

#[test]
fn profiles_keep_duplicate_foreign_and_original_byte_refusals() {
    for records in [
        "--encoding\tpcm16\n--encoding\tpcm24\n".to_owned(),
        "--future-encoding\tpcm16\n".to_owned(),
        "--encoding\tpcm16".to_owned(),
        format!("--encoding\t{}\n", "x".repeat(MAX_VALUE_BYTES + 1)),
        "--bind\t11:04\n".repeat(MAX_FIELDS + 1),
    ] {
        let encoded = format!("BEATKERNEL-NATIVE-PROFILE\t1\tlinux\n{records}").into_bytes();
        assert!(decode_profile(&encoded, HOST).is_err());
        assert!(decode_player_profile(&encoded, HOST).is_err());
    }
    let model = NativeSettings::from_args(&args(&["--encoding", "pcm24"]), HOST).unwrap();
    let v1 = encode_profile(&model, HOST).unwrap();
    let v2 = encode_player_profile(
        &PlayerProfile {
            native: model.clone(),
            presentation: PresentationSettings::default(),
        },
        HOST,
    )
    .unwrap();
    for host in [SettingsHost::Windows, SettingsHost::Macos] {
        assert!(decode_profile(&v1, host).is_err());
        assert!(decode_player_profile(&v1, host).is_err());
        assert!(decode_player_profile(&v2, host).is_err());
        assert!(encode_profile(&model, host).is_err());
    }
    for record in [
        "--encoding\tpcm32\n".to_owned(),
        "--future-encoding\tpcm24\n".to_owned(),
        format!("--bind\t{}\n", "x".repeat(MAX_VALUE_BYTES + 1)),
        "--bind\t11:04\n".repeat(MAX_FIELDS + 1),
    ] {
        let mut invalid = v2.clone();
        invalid.extend_from_slice(record.as_bytes());
        assert!(decode_player_profile(&invalid, HOST).is_err());
    }
    assert!(decode_player_profile(&v2[..v2.len() - 1], HOST).is_err());
    let oversized = vec![b'x'; MAX_PROFILE_BYTES + 1];
    assert!(decode_profile(&oversized, HOST).is_err());
    assert!(decode_player_profile(&oversized, HOST).is_err());
}

#[test]
fn native_defaults_and_watch_projection_forward_encoding_without_rewriting_source_options() {
    let defaults = NativeDefaults::for_validation();
    for token in TOKENS {
        let supplied = args(&[
            "--chart",
            "random.bms",
            "--encoding",
            token,
            "--alsa",
            "null",
            "--evdev",
            "/explicit",
            "--rate",
            "44100",
            "--channels",
            "1",
            "--period-frames",
            "64",
            "--buffer-frames",
            "512",
            "--speed",
            "3/2",
            "--chart-seed",
            "123",
            "--start-ns",
            "2000",
            "--end-ns",
            "4000",
            "--record-replay",
            "new.bkr",
            "--bind",
            "11:04",
        ]);
        let completed = complete(&supplied, HOST, &defaults).unwrap();
        let mut expected = supplied.clone();
        expected.extend(args(&["--channel-policy", "mono-stereo"]));
        assert_eq!(completed, expected);
        let mut watch = supplied.clone();
        watch.extend(args(&["--replay", "past.bkr"]));
        let projected = replay_args(&watch, HOST, &defaults).unwrap();
        assert_eq!(
            projected,
            args(&[
                "--chart",
                "random.bms",
                "--encoding",
                token,
                "--device",
                "null",
                "--rate",
                "44100",
                "--channels",
                "1",
                "--period-frames",
                "64",
                "--buffer-frames",
                "512",
                "--channel-policy",
                "mono-stereo",
                "--replay",
                "past.bkr",
            ])
        );
        assert_eq!(supplied[3], token);
        assert_eq!(value(&completed, "--speed"), Some("3/2"));
        assert_eq!(value(&completed, "--chart-seed"), Some("123"));
    }
    let completed = complete(&args(&["--evdev", "/explicit"]), HOST, &defaults).unwrap();
    assert!(value(&completed, "--encoding").is_none());
    let projected = replay_args(
        &args(&["--chart", "song.bms", "--replay", "old.bkr"]),
        HOST,
        &defaults,
    )
    .unwrap();
    assert!(value(&projected, "--encoding").is_none());
}

#[derive(Default)]
struct NoNativeDiscovery;

impl NativePreparationDevice for NoNativeDiscovery {
    fn default_output(&mut self) -> NativePreparationResult<String> {
        panic!("Linux encoding preparation must not discover another output")
    }
    fn output_format(&mut self, _: &str) -> NativePreparationResult<NativeOutputFormat> {
        panic!("Linux encoding preparation must keep the source grid supplied by the application")
    }
    fn keyboards(&mut self) -> NativePreparationResult<Vec<NativeKeyboardCandidate>> {
        panic!("explicit input/Watch must not discover a keyboard")
    }
    fn asio_format(&mut self, _: &[String]) -> NativePreparationResult<NativeOutputFormat> {
        panic!("Linux preparation must not discover ASIO")
    }
}

#[test]
fn prepare_play_and_primary_watch_replay_forward_chosen_encoding_after_real_profile_overlay() {
    for token in TOKENS {
        let original = args(&[
            "--encoding",
            "pcm16",
            "--rate",
            "44100",
            "--channels",
            "1",
            "--speed",
            "3/2",
            "--chart-seed",
            "123",
            "--evdev",
            "/explicit",
        ]);
        let model = NativeSettings::from_args(&original, HOST).unwrap();
        let bytes = encode_player_profile(
            &PlayerProfile {
                native: model,
                presentation: PresentationSettings::default(),
            },
            HOST,
        )
        .unwrap();
        let restored = decode_player_profile(&bytes, HOST).unwrap();
        let overlaid = overlay_native_args(
            &restored.native.native_args(),
            &args(&["--encoding", token]),
            HOST,
        )
        .unwrap();
        let mut play = args(&["--chart", "random.bms"]);
        play.extend(overlaid);
        let accepted = prepare(
            &play,
            HOST,
            PreparationMode::Play,
            &mut NoNativeDiscovery,
            |actual| {
                assert_eq!(actual, play);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(value(&accepted, "--encoding"), Some(token));
        assert_eq!(value(&accepted, "--rate"), Some("44100"));
        assert_eq!(value(&accepted, "--channels"), Some("1"));
        assert_eq!(value(&accepted, "--speed"), Some("3/2"));
        assert_eq!(value(&accepted, "--chart-seed"), Some("123"));
        let mut watch = play.clone();
        watch.extend(args(&["--replay", "past.bkr"]));
        let accepted = prepare(
            &watch,
            HOST,
            PreparationMode::Replay,
            &mut NoNativeDiscovery,
            |actual| {
                assert_eq!(actual, watch);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(value(&accepted, "--encoding"), Some(token));
        assert_eq!(value(&accepted, "--rate"), Some("44100"));
        assert_eq!(value(&accepted, "--channels"), Some("1"));
        assert_eq!(value(&accepted, "--replay"), Some("past.bkr"));
        for live_only in [
            "--speed",
            "--chart-seed",
            "--evdev",
            "--record-replay",
            "--bind",
        ] {
            assert!(value(&accepted, live_only).is_none());
        }
        assert_eq!(value(&watch, "--encoding"), Some(token));
        assert_eq!(value(&watch, "--speed"), Some("3/2"));
    }
}
