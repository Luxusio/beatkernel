//! Versioned native/player profiles; file work belongs outside real-time owners.
use crate::{
    presentation_settings::PresentationSettings,
    settings::{NativeSettings, SettingsHost},
};
#[path = "native_settings_profile.rs"]
mod native_storage;

pub use native_storage::{load_player_profile, load_profile, save_player_profile, save_profile};

pub const MAX_PROFILE_BYTES: usize = 72 * 1024;
const MAGIC: &str = "BEATKERNEL-NATIVE-PROFILE";

fn host_name(host: SettingsHost) -> &'static str {
    match host {
        SettingsHost::Windows => "windows",
        SettingsHost::Linux => "linux",
        SettingsHost::Macos => "macos",
    }
}

fn profile_native_args(values: &NativeSettings, host: SettingsHost) -> Result<Vec<String>, String> {
    // Empty schemas still identify their platform, preventing a foreign empty
    // model from being relabeled merely because native_args omits empty fields.
    let identity = match host {
        SettingsHost::Windows => "--keyboard-path",
        SettingsHost::Linux => "--evdev",
        SettingsHost::Macos => "--keyboard-registry",
    };
    if !values.fields().iter().any(|field| field.flag == identity) {
        return Err("profile native settings belong to another operating system".into());
    }
    let args = values.native_args();
    NativeSettings::from_args(&args, host)?;
    Ok(args)
}

/// Native-only, LF-terminated UTF-8; values already reject tabs/line breaks.
pub fn encode_profile(values: &NativeSettings, host: SettingsHost) -> Result<Vec<u8>, String> {
    let args = profile_native_args(values, host)?;
    let mut encoded = format!("{MAGIC}\t1\t{}\n", host_name(host));
    for pair in args.chunks_exact(2) {
        encoded.push_str(&pair[0]);
        encoded.push('\t');
        encoded.push_str(&pair[1]);
        encoded.push('\n');
    }
    if encoded.len() > MAX_PROFILE_BYTES {
        return Err("profile exceeds 72 KiB".into());
    }
    Ok(encoded.into_bytes())
}

/// Combined native/display profile; chart/library selection remains external.
#[derive(Clone, Debug)]
pub struct PlayerProfile {
    pub native: NativeSettings,
    pub presentation: PresentationSettings,
}

/// Encodes version 2 with all four canonical presentation records.
pub fn encode_player_profile(
    values: &PlayerProfile,
    host: SettingsHost,
) -> Result<Vec<u8>, String> {
    values.presentation.validate()?;
    let native = profile_native_args(&values.native, host)?;
    if native.len() / 2 > crate::settings::MAX_FIELDS {
        return Err("profile exceeds 128 native records".into());
    }
    let mut encoded = format!("{MAGIC}\t2\t{}\n", host_name(host));
    let display = values.presentation.args();
    for pair in native.chunks_exact(2).chain(display.chunks_exact(2)) {
        encoded.push_str(&pair[0]);
        encoded.push('\t');
        encoded.push_str(&pair[1]);
        encoded.push('\n');
    }
    if encoded.len() > MAX_PROFILE_BYTES {
        return Err("profile exceeds 72 KiB".into());
    }
    Ok(encoded.into_bytes())
}

/// Strict native-only version 1 decoder. Combined version 2 is refused.
pub fn decode_profile(encoded: &[u8], host: SettingsHost) -> Result<NativeSettings, String> {
    let (version, args) = profile_records(encoded, host)?;
    if version != 1 {
        return Err("native-only profile requires version 1".into());
    }
    NativeSettings::from_args(&args, host)
}

/// Decodes version 2 atomically, or upgrades version 1 with default display values.
/// All four version 2 presentation flags are required exactly once.
pub fn decode_player_profile(encoded: &[u8], host: SettingsHost) -> Result<PlayerProfile, String> {
    let (version, records) = profile_records(encoded, host)?;
    if version == 1 {
        return Ok(PlayerProfile {
            native: NativeSettings::from_args(&records, host)?,
            presentation: PresentationSettings::default(),
        });
    }
    let mut native = Vec::new();
    let mut display = Vec::new();
    for pair in records.chunks_exact(2) {
        let destination = if matches!(
            pair[0].as_str(),
            "--gpu-backend" | "--present" | "--ui-fps" | "--ui-lookahead-ms"
        ) {
            &mut display
        } else {
            &mut native
        };
        destination.extend(pair.iter().cloned());
    }
    if native.len() / 2 > crate::settings::MAX_FIELDS {
        return Err("profile exceeds 128 native records".into());
    }
    if display.len() != 8 {
        return Err("player profile requires all four presentation records".into());
    }
    let presentation = PresentationSettings::default().apply_overrides(&display)?;
    let native = NativeSettings::from_args(&native, host)?;
    Ok(PlayerProfile {
        native,
        presentation,
    })
}

/// Bounded UTF-8 record parsing shared by both schemas. CRLF is accepted.
fn profile_records(encoded: &[u8], host: SettingsHost) -> Result<(u8, Vec<String>), String> {
    if encoded.len() > MAX_PROFILE_BYTES {
        return Err("profile exceeds 72 KiB".into());
    }
    let text = std::str::from_utf8(encoded).map_err(|_| "profile is not UTF-8")?;
    if !text.ends_with('\n') {
        return Err("profile has an incomplete final record".into());
    }
    let (header, records) = text.split_once('\n').ok_or("profile header missing")?;
    let header = header.strip_suffix('\r').unwrap_or(header);
    let version = if header == format!("{MAGIC}\t1\t{}", host_name(host)) {
        1
    } else if header == format!("{MAGIC}\t2\t{}", host_name(host)) {
        2
    } else {
        return Err("profile magic, version or operating system differs".into());
    };
    let limit = if version == 1 {
        crate::settings::MAX_FIELDS
    } else {
        crate::settings::MAX_FIELDS + 4
    };
    let mut args = Vec::new();
    for record in records.split_terminator('\n') {
        if args.len() / 2 == limit {
            return Err("profile exceeds schema record limit".into());
        }
        let record = record.strip_suffix('\r').unwrap_or(record);
        let (flag, value) = record
            .split_once('\t')
            .ok_or("profile record requires flag and value")?;
        if flag == "--chart" {
            return Err("profile cannot own chart selection".into());
        }
        args.extend([flag.to_owned(), value.to_owned()]);
    }
    Ok((version, args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs, io,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
    fn values() -> NativeSettings {
        let args = [
            "--alsa",
            "device with spaces",
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--ghost-self",
            "내 기록.bkr",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        NativeSettings::from_args(&args, SettingsHost::Linux).unwrap()
    }
    #[test]
    fn combined_profiles_roundtrip_all_hosts_and_load_v1_display_defaults() {
        use crate::presentation_settings::{BackendChoice, Presentation};
        for host in [
            SettingsHost::Windows,
            SettingsHost::Linux,
            SettingsHost::Macos,
        ] {
            let native = NativeSettings::from_args(&[], host).unwrap();
            let model = PlayerProfile {
                native,
                presentation: PresentationSettings {
                    backend: BackendChoice::Vulkan,
                    presentation: Presentation::Mailbox,
                    fps: 90,
                    lookahead_ms: 3456,
                },
            };
            let encoded = encode_player_profile(&model, host).unwrap();
            assert!(String::from_utf8(encoded.clone())
                .unwrap()
                .starts_with(&format!("{MAGIC}\t2\t{}\n", host_name(host))));
            let decoded = decode_player_profile(&encoded, host).unwrap();
            assert_eq!(decoded.native.native_args(), model.native.native_args());
            assert_eq!(decoded.presentation, model.presentation);
            let crlf = String::from_utf8(encoded.clone())
                .unwrap()
                .replace('\n', "\r\n");
            assert_eq!(
                decode_player_profile(crlf.as_bytes(), host)
                    .unwrap()
                    .presentation,
                model.presentation
            );
            assert!(decode_profile(&encoded, host).is_err());
            assert!(decode_player_profile(&encoded, host).is_ok());
            let legacy = encode_profile(&model.native, host).unwrap();
            assert_eq!(
                decode_player_profile(&legacy, host).unwrap().presentation,
                PresentationSettings::default()
            );
            assert!(decode_player_profile(&legacy, host).is_ok());
        }
    }

    #[test]
    fn combined_profiles_preserve_unicode_repeats_and_stable_local_ids() {
        let args = [
            "--local-player",
            "4294967295:/dev/input/event0",
            "--local-player",
            "9:/dev/input/event1",
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--ghost-self",
            "내 기록.bkr",
        ]
        .map(String::from)
        .to_vec();
        let model = PlayerProfile {
            native: NativeSettings::from_args(&args, SettingsHost::Linux).unwrap(),
            presentation: PresentationSettings::default()
                .apply_overrides(&["--present".into(), "immediate".into()])
                .unwrap(),
        };
        let decoded = decode_player_profile(
            &encode_player_profile(&model, SettingsHost::Linux).unwrap(),
            SettingsHost::Linux,
        )
        .unwrap();
        assert_eq!(decoded.native.native_args(), args);
        assert_eq!(decoded.presentation, model.presentation);
        assert!(!decoded
            .native
            .native_args()
            .iter()
            .any(|flag| flag == "--present"));
    }

    #[test]
    fn v2_requires_four_distinct_display_fields_and_rejects_unknown_foreign_records() {
        let header = format!("{MAGIC}\t2\tlinux\n");
        let display =
            "--gpu-backend\tauto\n--present\tfifo\n--ui-fps\t120\n--ui-lookahead-ms\t2000\n";
        for records in [
            "",
            "--gpu-backend\tauto\n--present\tfifo\n--ui-fps\t120\n",
            "--gpu-backend\tauto\n--present\tfifo\n--ui-fps\t120\n--ui-fps\t90\n",
            "--gpu-backend\tauto\n--present\tfifo\n--ui-fps\t29\n--ui-lookahead-ms\t2000\n",
            "--gpu-backend\tauto\n--present\tfifo\n--ui-fps\t120\n--ui-lookahead-ms\t10001\n",
        ] {
            assert!(decode_player_profile(
                format!("{header}{records}").as_bytes(),
                SettingsHost::Linux
            )
            .is_err());
        }
        for forbidden in [
            "--chart\tchart.bms\n",
            "--library\tdirectory\n",
            "--unknown\tx\n",
            "--alsa\ta\n--alsa\tb\n",
            "--alsa\ta\tb\n",
            "\n",
        ] {
            assert!(decode_player_profile(
                format!("{header}{forbidden}{display}").as_bytes(),
                SettingsHost::Linux
            )
            .is_err());
        }
        let valid = format!("{header}{display}");
        assert!(decode_player_profile(valid.as_bytes(), SettingsHost::Windows).is_err());
        assert!(
            decode_player_profile(&valid.as_bytes()[..valid.len() - 1], SettingsHost::Linux)
                .is_err()
        );
        assert!(decode_player_profile(&[255, b'\n'], SettingsHost::Linux).is_err());
        assert!(decode_player_profile(
            format!("{MAGIC}\t3\tlinux\n{display}").as_bytes(),
            SettingsHost::Linux
        )
        .is_err());
        let foreign_blank = NativeSettings::from_args(&[], SettingsHost::Windows).unwrap();
        assert!(encode_profile(&foreign_blank, SettingsHost::Linux).is_err());
        assert!(encode_player_profile(
            &PlayerProfile {
                native: foreign_blank,
                presentation: PresentationSettings::default()
            },
            SettingsHost::Linux
        )
        .is_err());
    }

    #[test]
    fn combined_codec_checks_records_total_bytes_and_display_bounds_before_state() {
        let header = format!("{MAGIC}\t2\tlinux\n");
        let many = format!("{header}{}", "--bind\t11:04\n".repeat(133));
        assert!(profile_records(many.as_bytes(), SettingsHost::Linux).is_err());
        let native_over = format!(
            "{header}{}--gpu-backend\tauto\n--present\tfifo\n--ui-fps\t120\n",
            "--bind\t11:04\n".repeat(129)
        );
        assert!(decode_player_profile(native_over.as_bytes(), SettingsHost::Linux).is_err());
        assert!(
            decode_player_profile(&vec![b'x'; MAX_PROFILE_BYTES + 1], SettingsHost::Linux).is_err()
        );
        let model = PlayerProfile {
            native: values(),
            presentation: PresentationSettings {
                fps: 0,
                ..PresentationSettings::default()
            },
        };
        assert!(encode_player_profile(&model, SettingsHost::Linux).is_err());
        assert_eq!(model.presentation.fps, 0);
    }

    #[test]
    fn local_device_group_round_trips_and_overlays_as_one_ordered_group() {
        let original = [
            "--local-input",
            "/dev/input/event1",
            "--local-input",
            "/dev/input/event2",
            "--local-input",
            "/dev/input/event3",
            "--local-input",
            "/dev/input/event4",
        ]
        .map(String::from)
        .to_vec();
        let draft = NativeSettings::from_args(&original, SettingsHost::Linux).unwrap();
        let encoded = encode_profile(&draft, SettingsHost::Linux).unwrap();
        assert_eq!(
            decode_profile(&encoded, SettingsHost::Linux)
                .unwrap()
                .native_args(),
            original
        );
        let replacement = [
            "--local-input",
            "/dev/input/event5",
            "--local-input",
            "/dev/input/event6",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            crate::settings::overlay_native_args(&original, &replacement, SettingsHost::Linux)
                .unwrap(),
            replacement
        );
    }
    #[test]
    fn utf8_pairs_and_repeat_order_roundtrip_with_lf_and_crlf() {
        let model = values();
        let encoded = encode_profile(&model, SettingsHost::Linux).unwrap();
        assert_eq!(
            decode_profile(&encoded, SettingsHost::Linux)
                .unwrap()
                .native_args(),
            model.native_args()
        );
        let crlf = String::from_utf8(encoded).unwrap().replace('\n', "\r\n");
        assert_eq!(
            decode_profile(crlf.as_bytes(), SettingsHost::Linux)
                .unwrap()
                .native_args(),
            model.native_args()
        );
        assert!(decode_profile(crlf.as_bytes(), SettingsHost::Windows).is_err());
    }
    #[test]
    fn malformed_unknown_and_oversized_profiles_do_not_produce_state() {
        let header = "BEATKERNEL-NATIVE-PROFILE\t1\tlinux\n";
        for record in [
            "--alsa\t",
            "--chart\tx.bms\n",
            "--unknown\tx\n",
            "--alsa\ta\n--alsa\tb\n",
            "--alsa\ta\tb\n",
            "\n",
        ] {
            assert!(
                decode_profile(format!("{header}{record}").as_bytes(), SettingsHost::Linux)
                    .is_err()
            );
        }
        assert!(decode_profile(
            b"BEATKERNEL-NATIVE-PROFILE\t2\tlinux\n",
            SettingsHost::Linux
        )
        .is_err());
        assert!(decode_profile(&[255, b'\n'], SettingsHost::Linux).is_err());
        assert!(decode_profile(&vec![b'x'; MAX_PROFILE_BYTES + 1], SettingsHost::Linux).is_err());
    }
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let root = std::env::temp_dir();
            for _ in 0..32 {
                let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
                let path = root.join(format!(
                    "beatkernel-profile-test-{}-{id}",
                    std::process::id()
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("profile fixture directory: {error}"),
                }
            }
            panic!("profile fixture collision limit");
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn saving_replaces_profiles_and_preserves_foreign_files_without_temp_leaks() {
        let directory = Directory::new();
        let path = directory.0.join("native.profile");
        let mut model = values();
        save_profile(&path, &model, SettingsHost::Linux).unwrap();
        assert_eq!(
            load_profile(&path, SettingsHost::Linux)
                .unwrap()
                .native_args(),
            model.native_args()
        );
        model.set_value(0, "hw:1").unwrap();
        save_profile(&path, &model, SettingsHost::Linux).unwrap();
        assert_eq!(
            load_profile(&path, SettingsHost::Linux)
                .unwrap()
                .native_args(),
            model.native_args()
        );
        fs::write(&path, b"foreign file").unwrap();
        assert!(save_profile(&path, &model, SettingsHost::Linux).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"foreign file");
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn symlink_profile_is_not_loaded_or_replaced() {
        let directory = Directory::new();
        let target = directory.0.join("target");
        fs::write(
            &target,
            encode_profile(&values(), SettingsHost::Linux).unwrap(),
        )
        .unwrap();
        let link = directory.0.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(load_profile(&link, SettingsHost::Linux).is_err());
        assert!(save_profile(&link, &values(), SettingsHost::Linux).is_err());
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }
}

#[cfg(test)]
mod chart_seed_profile_fixtures {
    use super::*;
    #[test]
    fn existing_profile_schemas_roundtrip_source_seeds_for_every_host() {
        for host in [
            SettingsHost::Windows,
            SettingsHost::Linux,
            SettingsHost::Macos,
        ] {
            for seed in ["0", "0003", "18446744073709551615"] {
                let native =
                    NativeSettings::from_args(&["--chart-seed".into(), seed.into()], host).unwrap();
                let v1 = encode_profile(&native, host).unwrap();
                let restored = decode_profile(&v1, host).unwrap();
                assert_eq!(restored.native_args(), native.native_args());
                assert_eq!(restored.chart_seed().unwrap(), seed.parse::<u64>().unwrap());
                let v2 = encode_player_profile(
                    &PlayerProfile {
                        native,
                        presentation: PresentationSettings::default(),
                    },
                    host,
                )
                .unwrap();
                let restored = decode_player_profile(&v2, host).unwrap();
                assert_eq!(
                    restored.native.chart_seed().unwrap(),
                    seed.parse::<u64>().unwrap()
                );
                assert_eq!(encode_player_profile(&restored, host).unwrap(), v2);
            }
        }
    }
}
