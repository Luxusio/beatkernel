//! Versioned native profiles; file work belongs outside real-time owners.
use crate::settings::{NativeSettings, SettingsHost};
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

/// Native-only, LF-terminated UTF-8; values already reject tabs/line breaks.
pub fn encode_profile(values: &NativeSettings, host: SettingsHost) -> Result<Vec<u8>, String> {
    let args = values.native_args();
    // Ensure a caller did not supply a model for another platform.
    NativeSettings::from_args(&args, host)?;
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

/// Checks host/schema and draft limits before returning any replacement state.
/// CRLF records are accepted; the encoder consistently produces LF records.
pub fn decode_profile(encoded: &[u8], host: SettingsHost) -> Result<NativeSettings, String> {
    if encoded.len() > MAX_PROFILE_BYTES {
        return Err("profile exceeds 72 KiB".into());
    }
    let text = std::str::from_utf8(encoded).map_err(|_| "profile is not UTF-8")?;
    if !text.ends_with('\n') {
        return Err("profile has an incomplete final record".into());
    }
    let (header, records) = text.split_once('\n').ok_or("profile header missing")?;
    let expected = format!("{MAGIC}\t1\t{}", host_name(host));
    if header.strip_suffix('\r').unwrap_or(header) != expected {
        return Err("profile magic, version or operating system differs".into());
    }
    let mut args = Vec::new();
    for record in records.split_terminator('\n') {
        if args.len() / 2 == crate::settings::MAX_FIELDS {
            return Err("profile exceeds 128 records".into());
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
    NativeSettings::from_args(&args, host)
}

/// Reads a regular, non-symlink file with a growth-safe encoded byte limit.
/// Schema loading alone does not validate native resource availability.
pub fn load_profile(path: &Path, host: SettingsHost) -> Result<NativeSettings, Box<dyn Error>> {
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
    Ok(decode_profile(&bytes, host)?)
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
    if path.file_name().is_none() {
        return Err("profile path requires a file name".into());
    }
    let replacing = match fs::symlink_metadata(path) {
        Ok(_) => {
            load_profile(path, host)?;
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
        file.write_all(&encoded)?;
        file.sync_all()
    })();
    drop(file); // Windows rename/cleanup must not depend on an open handle.
    written?;
    if replacing {
        // Catch a changed/foreign/symlink target before replacement. Concurrent
        // writers still require external coordination; this is not a lock.
        load_profile(path, host)?;
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
