//! Native speed selection uses the existing bounded draft and profile paths.
use beatkernel_bms_runtime::{
    presentation_settings::PresentationSettings,
    settings::{
        overlay_native_args, parse_playback_speed, NativeSettings, SettingsHost, MAX_FIELDS,
        MAX_TOTAL_BYTES, MAX_VALUE_BYTES,
    },
    settings_profile::{
        decode_player_profile, decode_profile, encode_player_profile, encode_profile,
        PlayerProfile, MAX_PROFILE_BYTES,
    },
};

const HOSTS: [SettingsHost; 3] = [
    SettingsHost::Windows,
    SettingsHost::Linux,
    SettingsHost::Macos,
];

fn args(tokens: &[&str]) -> Vec<String> {
    tokens.iter().map(|token| (*token).to_owned()).collect()
}

fn host_name(host: SettingsHost) -> &'static str {
    match host {
        SettingsHost::Windows => "windows",
        SettingsHost::Linux => "linux",
        SettingsHost::Macos => "macos",
    }
}

fn ratio(model: &NativeSettings) -> (i64, u64) {
    let rate = model.playback_speed().unwrap().rate();
    (rate.numerator(), rate.denominator())
}

fn speed_row(model: &NativeSettings) -> usize {
    model
        .fields()
        .iter()
        .position(|row| row.flag == "--speed")
        .unwrap()
}

#[test]
fn empty_speed_preserves_all_host_defaults_and_empty_native_args() {
    for host in HOSTS {
        let model = NativeSettings::from_args(&[], host).unwrap();
        assert_eq!(ratio(&model), (1, 1));
        assert!(model.native_args().is_empty());
        assert!(model.fields()[speed_row(&model)].value.is_empty());
    }
}

#[test]
fn strict_parser_normalizes_four_rates_and_leading_zero_fraction() {
    for (text, expected) in [
        ("1/2", (1, 2)),
        ("1/1", (1, 1)),
        ("2/1", (2, 1)),
        ("3/2", (3, 2)),
        ("0006/0004", (3, 2)),
    ] {
        let rate = parse_playback_speed(text).unwrap().rate();
        assert_eq!((rate.numerator(), rate.denominator()), expected, "{text}");
        for host in HOSTS {
            let model = NativeSettings::from_args(&args(&["--speed", text]), host).unwrap();
            assert_eq!(ratio(&model), expected);
            assert_eq!(model.native_args(), args(&["--speed", text]));
        }
    }
}

#[test]
fn strict_parser_accepts_maximal_components_without_narrowing_denominator() {
    let rate = parse_playback_speed("9223372036854775807/18446744073709551615")
        .unwrap()
        .rate();
    assert_eq!(rate.numerator(), i64::MAX);
    assert_eq!(rate.denominator(), u64::MAX);
    assert_eq!(
        parse_playback_speed("9223372036854775807/9223372036854775807")
            .unwrap()
            .rate()
            .numerator(),
        1
    );
}

#[test]
fn strict_parser_refuses_incomplete_nonpositive_non_ascii_and_overflow_values() {
    for text in [
        "",
        "1",
        "1/",
        "/1",
        "0/1",
        "1/0",
        "-1/2",
        "1/-2",
        "+1/2",
        "1/+2",
        "1.5/1",
        "1e2/1",
        "1/2/3",
        " 1/2",
        "1/2 ",
        "１/２",
        "1\n/2",
        "9223372036854775808/1",
        "1/18446744073709551616",
    ] {
        assert!(parse_playback_speed(text).is_err(), "accepted {text:?}");
    }
}

#[test]
fn incomplete_edit_is_retained_until_typed_launch_validation_then_can_recover() {
    for host in HOSTS {
        let mut model = NativeSettings::from_args(&[], host).unwrap();
        let row = speed_row(&model);
        for draft in ["3/", "0/1", "-1/2", "garbage"] {
            model.set_value(row, draft).unwrap();
            assert_eq!(model.fields()[row].value, draft);
            assert!(model.playback_speed().is_err());
        }
        model.set_value(row, "6/4").unwrap();
        assert_eq!(ratio(&model), (3, 2));
        model.set_value(row, "").unwrap();
        assert_eq!(ratio(&model), (1, 1));
        assert!(model.native_args().is_empty());
    }
}

#[test]
fn duplicate_unknown_and_unpaired_flags_keep_existing_refusal_contract() {
    for host in HOSTS {
        for tokens in [
            args(&["--speed", "1/2", "--speed", "2/1"]),
            args(&["--unknown-speed", "1/2"]),
            args(&["--speed"]),
        ] {
            assert!(NativeSettings::from_args(&tokens, host).is_err());
        }
    }
}

#[test]
fn oversized_or_control_draft_refusal_is_atomic_and_input_bounded() {
    for host in HOSTS {
        let mut model = NativeSettings::from_args(&args(&["--speed", "3/2"]), host).unwrap();
        let row = speed_row(&model);
        let oversized = "1".repeat(MAX_VALUE_BYTES + 1);
        for text in [oversized.as_str(), "1\t/2", "1\n/2", "1\u{2028}/2"] {
            assert!(model.set_value(row, text).is_err());
            assert_eq!(model.native_args(), args(&["--speed", "3/2"]));
            assert!(NativeSettings::from_args(&args(&["--speed", text]), host).is_err());
        }
        let too_many: Vec<String> = (0..=MAX_FIELDS)
            .flat_map(|_| args(&["--bind", "11:04"]))
            .collect();
        assert!(NativeSettings::from_args(&too_many, host).is_err());
        let too_large: Vec<String> = (0..=(MAX_TOTAL_BYTES / MAX_VALUE_BYTES))
            .flat_map(|_| vec!["--bind".to_owned(), "x".repeat(MAX_VALUE_BYTES)])
            .collect();
        assert!(NativeSettings::from_args(&too_large, host).is_err());
    }
}

#[test]
fn speed_appends_without_shifting_existing_native_field_ids() {
    for (host, platform_rows) in [
        (SettingsHost::Windows, 16),
        (SettingsHost::Linux, 9),
        (SettingsHost::Macos, 7),
    ] {
        let model = NativeSettings::from_args(&[], host).unwrap();
        assert_eq!(model.fields()[platform_rows].flag, "--timing-preset");
        assert_eq!(model.fields()[platform_rows + 3].flag, "--chart-seed");
        assert_eq!(model.fields()[platform_rows + 6].flag, "--bind");
        assert_eq!(model.fields()[platform_rows + 30].flag, "--mp-server-name");
        assert_eq!(speed_row(&model), platform_rows + 31);
        assert_eq!(model.fields().last().unwrap().flag, "--speed");
    }
}

#[test]
fn explicit_speed_preserves_native_argument_order_and_unrelated_options() {
    for host in HOSTS {
        let supplied = args(&[
            "--bind",
            "11:04",
            "--speed",
            "0006/0004",
            "--seconds",
            "12",
            "--bind",
            "12:05",
        ]);
        let model = NativeSettings::from_args(&supplied, host).unwrap();
        assert_eq!(model.native_args(), supplied);
        assert_eq!(ratio(&model), (3, 2));
    }
}

#[test]
fn speed_overlay_replaces_exact_flag_and_empty_override_restores_default() {
    for host in HOSTS {
        let base = args(&["--speed", "1/2", "--bind", "11:04", "--seconds", "10"]);
        let merged = overlay_native_args(&base, &args(&["--speed", "6/4"]), host).unwrap();
        assert_eq!(
            merged,
            args(&["--bind", "11:04", "--seconds", "10", "--speed", "6/4"])
        );
        assert_eq!(
            ratio(&NativeSettings::from_args(&merged, host).unwrap()),
            (3, 2)
        );
        assert_eq!(
            overlay_native_args(&base, &args(&["--speed", ""]), host).unwrap(),
            args(&["--bind", "11:04", "--seconds", "10"])
        );
        assert!(
            overlay_native_args(&base, &args(&["--speed", "1/1", "--speed", "2/1"]), host).is_err()
        );
    }
}

#[test]
fn default_profile_v1_bytes_remain_exact_and_v2_has_no_speed_record() {
    for host in HOSTS {
        let model = NativeSettings::from_args(&[], host).unwrap();
        assert_eq!(
            encode_profile(&model, host).unwrap(),
            format!("BEATKERNEL-NATIVE-PROFILE\t1\t{}\n", host_name(host)).into_bytes()
        );
        let profile = PlayerProfile {
            native: model,
            presentation: PresentationSettings::default(),
        };
        let encoded = encode_player_profile(&profile, host).unwrap();
        let text = String::from_utf8(encoded.clone()).unwrap();
        assert!(!text.contains("--speed"));
        assert_eq!(
            ratio(&decode_player_profile(&encoded, host).unwrap().native),
            (1, 1)
        );
        // The old v2 byte representation is exactly its unchanged presentation pairs.
        let mut expected = format!("BEATKERNEL-NATIVE-PROFILE\t2\t{}\n", host_name(host));
        for pair in profile.presentation.args().chunks_exact(2) {
            expected.push_str(&format!("{}\t{}\n", pair[0], pair[1]));
        }
        assert_eq!(encoded, expected.into_bytes());
    }
}

#[test]
fn real_v1_and_v2_profiles_roundtrip_fraction_tokens_and_typed_identity() {
    for host in HOSTS {
        for fraction in ["1/2", "1/1", "2/1", "0006/0004"] {
            let model =
                NativeSettings::from_args(&args(&["--speed", fraction, "--bind", "11:04"]), host)
                    .unwrap();
            let encoded = encode_profile(&model, host).unwrap();
            assert_eq!(
                encoded,
                format!(
                    "BEATKERNEL-NATIVE-PROFILE\t1\t{}\n--speed\t{fraction}\n--bind\t11:04\n",
                    host_name(host)
                )
                .into_bytes()
            );
            let restored = decode_profile(&encoded, host).unwrap();
            assert_eq!(restored.native_args(), model.native_args());
            assert_eq!(ratio(&restored), ratio(&model));
            assert_eq!(
                ratio(&decode_player_profile(&encoded, host).unwrap().native),
                ratio(&model)
            );
            let profile = PlayerProfile {
                native: model,
                presentation: PresentationSettings::default(),
            };
            let encoded = encode_player_profile(&profile, host).unwrap();
            let restored = decode_player_profile(&encoded, host).unwrap();
            assert_eq!(restored.native.native_args(), profile.native.native_args());
            assert_eq!(restored.presentation, profile.presentation);
            assert!(decode_profile(&encoded, host).is_err());
        }
    }
}

#[test]
fn profile_draft_keeps_invalid_fraction_until_typed_validation() {
    for host in HOSTS {
        let model = NativeSettings::from_args(&args(&["--speed", "1/"]), host).unwrap();
        let encoded = encode_profile(&model, host).unwrap();
        let restored = decode_profile(&encoded, host).unwrap();
        assert_eq!(restored.native_args(), args(&["--speed", "1/"]));
        assert!(restored.playback_speed().is_err());
        let encoded = encode_player_profile(
            &PlayerProfile {
                native: model,
                presentation: PresentationSettings::default(),
            },
            host,
        )
        .unwrap();
        assert!(decode_player_profile(&encoded, host)
            .unwrap()
            .native
            .playback_speed()
            .is_err());
    }
}

#[test]
fn malformed_profiles_keep_original_bounds_duplicate_and_unknown_flag_refusals() {
    for host in HOSTS {
        let header = format!("BEATKERNEL-NATIVE-PROFILE\t1\t{}\n", host_name(host));
        for records in [
            "--speed\t1/2\n--speed\t2/1\n".to_owned(),
            "--future-speed\t1/2\n".to_owned(),
            "--speed\t1/2".to_owned(),
            format!("--speed\t{}\n", "1".repeat(MAX_VALUE_BYTES + 1)),
            "--bind\t11:04\n".repeat(MAX_FIELDS + 1),
        ] {
            let encoded = format!("{header}{records}").into_bytes();
            assert!(decode_profile(&encoded, host).is_err());
            assert!(decode_player_profile(&encoded, host).is_err());
        }
        assert!(decode_profile(&vec![b'x'; MAX_PROFILE_BYTES + 1], host).is_err());
        let valid_v2 = encode_player_profile(
            &PlayerProfile {
                native: NativeSettings::from_args(&args(&["--speed", "1/2"]), host).unwrap(),
                presentation: PresentationSettings::default(),
            },
            host,
        )
        .unwrap();
        for record in [
            "--speed\t2/1\n".to_owned(),
            "--future-speed\t1/2\n".to_owned(),
            format!("--bind\t{}\n", "x".repeat(MAX_VALUE_BYTES + 1)),
            "--bind\t11:04\n".repeat(MAX_FIELDS + 1),
        ] {
            let mut invalid = valid_v2.clone();
            invalid.extend_from_slice(record.as_bytes());
            assert!(decode_player_profile(&invalid, host).is_err());
        }
        assert!(decode_player_profile(&valid_v2[..valid_v2.len() - 1], host).is_err());
        assert!(decode_player_profile(&vec![b'x'; MAX_PROFILE_BYTES + 1], host).is_err());
        let foreign = if host == SettingsHost::Linux {
            SettingsHost::Windows
        } else {
            SettingsHost::Linux
        };
        assert!(decode_profile(header.as_bytes(), foreign).is_err());
    }
}
