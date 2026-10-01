//! Versioned native/player profiles; file work belongs outside real-time owners.
use crate::{
    presentation_settings::PresentationSettings,
    settings::{NativeSettings, SettingsHost},
};
use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_PROFILE_BYTES: usize = 72 * 1024;
const MAGIC: &str = "BEATKERNEL-NATIVE-PROFILE";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

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

/// Reads a regular, non-symlink version 1 native profile with a growth-safe cap.
pub fn load_profile(path: &Path, host: SettingsHost) -> Result<NativeSettings, Box<dyn Error>> {
    Ok(decode_profile(&read_profile_bytes(path)?, host)?)
}

/// Reads a combined player profile or a legacy native profile with display defaults.
pub fn load_player_profile(
    path: &Path,
    host: SettingsHost,
) -> Result<PlayerProfile, Box<dyn Error>> {
    Ok(decode_player_profile(&read_profile_bytes(path)?, host)?)
}

fn read_profile_bytes(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    regular_path(path)?;
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_PROFILE_BYTES as u64 {
        return Err("profile must be a regular file at most 72 KiB".into());
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(MAX_PROFILE_BYTES + 1)?;
    file.take((MAX_PROFILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err("profile exceeds 72 KiB".into());
    }
    Ok(bytes)
}

/// Writes a synced, uniquely owned sibling before publishing it.
/// Existing targets must be structurally valid same-host profiles. A new target
/// is published with create-only hard linking; replacements use rename. Neither
/// path promises interprocess locking or directory crash durability.
pub fn save_profile(
    path: &Path,
    values: &NativeSettings,
    host: SettingsHost,
) -> Result<(), Box<dyn Error>> {
    let encoded = encode_profile(values, host)?;
    save_encoded(path, &encoded, host, validate_native_profile)
}

/// Writes version 2 using the same bounded file owner and publication protocol.
/// Existing valid same-host version 1 or version 2 may be replaced; malformed,
/// foreign and symlink targets remain refused. Native-only save refuses version 2.
pub fn save_player_profile(
    path: &Path,
    values: &PlayerProfile,
    host: SettingsHost,
) -> Result<(), Box<dyn Error>> {
    let encoded = encode_player_profile(values, host)?;
    save_encoded(path, &encoded, host, validate_player_profile)
}

fn validate_native_profile(bytes: &[u8], host: SettingsHost) -> Result<(), String> {
    decode_profile(bytes, host).map(|_| ())
}
fn validate_player_profile(bytes: &[u8], host: SettingsHost) -> Result<(), String> {
    decode_player_profile(bytes, host).map(|_| ())
}
fn save_encoded(
    path: &Path,
    encoded: &[u8],
    host: SettingsHost,
    validate: fn(&[u8], SettingsHost) -> Result<(), String>,
) -> Result<(), Box<dyn Error>> {
    if path.file_name().is_none() {
        return Err("profile path requires a file name".into());
    }
    let replacing = match fs::symlink_metadata(path) {
        Ok(_) => {
            validate(&read_profile_bytes(path)?, host)?;
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (mut file, mut temporary) = create_temporary(parent)?;
    let written = (|| -> io::Result<()> {
        file.write_all(encoded)?;
        file.sync_all()
    })();
    drop(file); // Windows rename/cleanup must not depend on an open handle.
    written?;
    if replacing {
        // Catch a changed/foreign/symlink target before replacement. Concurrent
        // writers still require external coordination; this is not a lock.
        validate(&read_profile_bytes(path)?, host)?;
        fs::rename(temporary.path(), path)?;
        temporary.path = None;
    } else {
        // Unlike rename, publication cannot replace a target created concurrently.
        // Unsupported hard links fail explicitly without a clobbering fallback.
        fs::hard_link(temporary.path(), path)?;
    }
    Ok(())
}

fn regular_path(path: &Path) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("profile path must be a regular file without symlinks".into());
    }
    Ok(())
}
struct OwnedTemporary {
    path: Option<PathBuf>,
}
impl OwnedTemporary {
    fn path(&self) -> &Path {
        self.path.as_deref().expect("live owned temporary path")
    }
}
impl Drop for OwnedTemporary {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path);
        }
    }
}
fn create_temporary(parent: &Path) -> Result<(File, OwnedTemporary), Box<dyn Error>> {
    for _ in 0..32 {
        let id = NEXT_TEMP
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "profile temporary identity exhausted")?;
        let path = parent.join(format!(
            ".beatkernel-profile-{}-{id}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, OwnedTemporary { path: Some(path) })),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err("profile temporary collision limit reached".into())
}

#[cfg(test)]
mod tests {
    use super::*;
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
            assert!(
                String::from_utf8(encoded.clone())
                    .unwrap()
                    .starts_with(&format!("{MAGIC}\t2\t{}\n", host_name(host)))
            );
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
            assert!(validate_native_profile(&encoded, host).is_err()); // Old save protocol cannot overwrite v2.
            assert!(validate_player_profile(&encoded, host).is_ok());
            let legacy = encode_profile(&model.native, host).unwrap();
            assert_eq!(
                decode_player_profile(&legacy, host).unwrap().presentation,
                PresentationSettings::default()
            );
            assert!(validate_player_profile(&legacy, host).is_ok());
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
        assert!(
            !decoded
                .native
                .native_args()
                .iter()
                .any(|flag| flag == "--present")
        );
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
            assert!(
                decode_player_profile(format!("{header}{records}").as_bytes(), SettingsHost::Linux)
                    .is_err()
            );
        }
        for forbidden in [
            "--chart\tchart.bms\n",
            "--library\tdirectory\n",
            "--unknown\tx\n",
            "--alsa\ta\n--alsa\tb\n",
            "--alsa\ta\tb\n",
            "\n",
        ] {
            assert!(
                decode_player_profile(
                    format!("{header}{forbidden}{display}").as_bytes(),
                    SettingsHost::Linux
                )
                .is_err()
            );
        }
        let valid = format!("{header}{display}");
        assert!(decode_player_profile(valid.as_bytes(), SettingsHost::Windows).is_err());
        assert!(
            decode_player_profile(&valid.as_bytes()[..valid.len() - 1], SettingsHost::Linux)
                .is_err()
        );
        assert!(decode_player_profile(&[255, b'\n'], SettingsHost::Linux).is_err());
        assert!(
            decode_player_profile(
                format!("{MAGIC}\t3\tlinux\n{display}").as_bytes(),
                SettingsHost::Linux
            )
            .is_err()
        );
        let foreign_blank = NativeSettings::from_args(&[], SettingsHost::Windows).unwrap();
        assert!(encode_profile(&foreign_blank, SettingsHost::Linux).is_err());
        assert!(
            encode_player_profile(
                &PlayerProfile {
                    native: foreign_blank,
                    presentation: PresentationSettings::default()
                },
                SettingsHost::Linux
            )
            .is_err()
        );
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
        assert!(
            decode_profile(
                b"BEATKERNEL-NATIVE-PROFILE\t2\tlinux\n",
                SettingsHost::Linux
            )
            .is_err()
        );
        assert!(decode_profile(&[255, b'\n'], SettingsHost::Linux).is_err());
        assert!(decode_profile(&vec![b'x'; MAX_PROFILE_BYTES + 1], SettingsHost::Linux).is_err());
    }
    // Authored file regression scenarios; do not execute while QA is deferred.
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let root = std::env::temp_dir();
            for _ in 0..32 {
                let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
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
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
