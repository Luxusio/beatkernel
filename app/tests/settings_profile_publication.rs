use beatkernel_bms_runtime::{
    presentation_settings::{BackendChoice, Presentation, PresentationSettings},
    settings::{NativeSettings, SettingsHost},
    settings_profile::{
        encode_player_profile, encode_profile, load_player_profile, load_profile,
        save_player_profile, save_profile, PlayerProfile, MAX_PROFILE_BYTES,
    },
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..32 {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-public-profile-publication-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("fixture directory: {error}"),
            }
        }
        panic!("fixture directory collisions");
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn entries(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn native(device: &str) -> NativeSettings {
    NativeSettings::from_args(&["--alsa".into(), device.into()], SettingsHost::Linux).unwrap()
}
fn player(device: &str) -> PlayerProfile {
    PlayerProfile {
        native: native(device),
        presentation: PresentationSettings {
            backend: BackendChoice::Vulkan,
            presentation: Presentation::Mailbox,
            fps: 90,
            lookahead_ms: 3456,
        },
    }
}
fn invalid_input(error: Box<dyn std::error::Error>) {
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn native_v1_create_replace_and_reload_preserve_canonical_bytes() {
    let dir = Directory::new();
    let path = dir.path("native.profile");
    for device in ["first device", "새 장치"] {
        let model = native(device);
        save_profile(&path, &model, SettingsHost::Linux).unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            encode_profile(&model, SettingsHost::Linux).unwrap()
        );
        assert_eq!(
            load_profile(&path, SettingsHost::Linux)
                .unwrap()
                .native_args(),
            model.native_args()
        );
        let upgraded = load_player_profile(&path, SettingsHost::Linux).unwrap();
        assert_eq!(upgraded.native.native_args(), model.native_args());
        assert_eq!(upgraded.presentation, PresentationSettings::default());
        assert_eq!(dir.entries(), vec![path.clone()]);
    }
}

#[test]
fn player_v2_creates_replaces_upgrades_v1_and_refuses_native_downgrade() {
    let dir = Directory::new();
    let path = dir.path("player.profile");
    // Direct v2 create and replacement exercise the combined public adapter.
    for device in ["first", "second"] {
        let model = player(device);
        save_player_profile(&path, &model, SettingsHost::Linux).unwrap();
        let bytes = encode_player_profile(&model, SettingsHost::Linux).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let loaded = load_player_profile(&path, SettingsHost::Linux).unwrap();
        assert_eq!(loaded.native.native_args(), model.native.native_args());
        assert_eq!(loaded.presentation, model.presentation);
        assert!(load_profile(&path, SettingsHost::Linux).is_err());
        assert!(save_profile(&path, &native("downgrade"), SettingsHost::Linux).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(dir.entries(), vec![path.clone()]);
    }
    let legacy = dir.path("legacy.profile");
    save_profile(&legacy, &native("legacy"), SettingsHost::Linux).unwrap();
    let model = player("upgraded");
    save_player_profile(&legacy, &model, SettingsHost::Linux).unwrap();
    assert_eq!(
        fs::read(&legacy).unwrap(),
        encode_player_profile(&model, SettingsHost::Linux).unwrap()
    );
    assert_eq!(
        load_player_profile(&legacy, SettingsHost::Linux)
            .unwrap()
            .presentation,
        model.presentation
    );
}

#[test]
fn old_profile_stage_like_final_name_is_a_valid_complete_public_profile() {
    let dir = Directory::new();
    let path = dir.path(&format!(".beatkernel-profile-{}-0.tmp", std::process::id()));
    let model = native("previously collided");
    save_profile(&path, &model, SettingsHost::Linux).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        encode_profile(&model, SettingsHost::Linux).unwrap()
    );
    assert_eq!(
        load_profile(&path, SettingsHost::Linux)
            .unwrap()
            .native_args(),
        model.native_args()
    );
    assert_eq!(dir.entries(), vec![path]);
}

#[test]
fn reserved_staging_names_and_native_aliases_refuse_before_target_io() {
    let dir = Directory::new();
    for name in [
        "0123ABCD.456",
        "abcdefab.cde",
        " 0123ABCD.456",
        "0123ABCD.456.",
        "0123ABCD.456:stream",
    ] {
        let path = dir.path(name);
        invalid_input(save_profile(&path, &native("new"), SettingsHost::Linux).unwrap_err());
        invalid_input(save_player_profile(&path, &player("new"), SettingsHost::Linux).unwrap_err());
        let missing_parent = dir.path("missing-parent").join(name);
        invalid_input(
            save_profile(&missing_parent, &native("new"), SettingsHost::Linux).unwrap_err(),
        );
        invalid_input(
            save_player_profile(&missing_parent, &player("new"), SettingsHost::Linux).unwrap_err(),
        );
        assert!(dir.entries().is_empty());
    }
}

#[test]
fn existing_reserved_malformed_target_remains_unchanged() {
    let dir = Directory::new();
    let path = dir.path("ABCDEF01.234");
    fs::write(&path, b"foreign reserved contents").unwrap();
    invalid_input(save_profile(&path, &native("new"), SettingsHost::Linux).unwrap_err());
    invalid_input(save_player_profile(&path, &player("new"), SettingsHost::Linux).unwrap_err());
    assert_eq!(fs::read(&path).unwrap(), b"foreign reserved contents");
    assert_eq!(dir.entries(), vec![path]);
}

#[test]
fn encoding_errors_precede_reserved_names_and_preserve_existing_files() {
    let dir = Directory::new();
    let path = dir.path("ABCDEF01.234");
    fs::write(&path, b"preserved").unwrap();
    let foreign = NativeSettings::from_args(&[], SettingsHost::Windows).unwrap();
    let expected = encode_profile(&foreign, SettingsHost::Linux).unwrap_err();
    let error = save_profile(&path, &foreign, SettingsHost::Linux).unwrap_err();
    assert_eq!(error.to_string(), expected);
    assert!(error.downcast_ref::<io::Error>().is_none());
    let mut invalid = player("new");
    invalid.presentation.fps = 0;
    let expected = encode_player_profile(&invalid, SettingsHost::Linux).unwrap_err();
    assert_eq!(
        save_player_profile(&path, &invalid, SettingsHost::Linux)
            .unwrap_err()
            .to_string(),
        expected
    );
    assert_eq!(fs::read(&path).unwrap(), b"preserved");
    assert_eq!(dir.entries(), vec![path]);
}

#[test]
fn missing_filename_and_missing_parent_refuse_without_publication() {
    let dir = Directory::new();
    let model = native("new");
    let error = save_profile(Path::new("/"), &model, SettingsHost::Linux).unwrap_err();
    assert_eq!(error.to_string(), "profile path requires a file name");
    assert!(save_player_profile(Path::new("/"), &player("new"), SettingsHost::Linux).is_err());
    let path = dir.path("missing-parent").join("profile");
    assert_eq!(
        save_profile(&path, &model, SettingsHost::Linux)
            .unwrap_err()
            .downcast_ref::<io::Error>()
            .unwrap()
            .kind(),
        io::ErrorKind::NotFound
    );
    assert!(dir.entries().is_empty());
}

#[test]
fn malformed_truncated_oversized_and_wrong_host_files_refuse_load_and_save_unchanged() {
    for bytes in [
        b"foreign data".to_vec(),
        b"BEATKERNEL-NATIVE-PROFILE\t1\tlinux".to_vec(),
        b"BEATKERNEL-NATIVE-PROFILE\t1\twindows\n".to_vec(),
        vec![b'x'; MAX_PROFILE_BYTES + 1],
    ] {
        let dir = Directory::new();
        let path = dir.path("invalid.profile");
        fs::write(&path, &bytes).unwrap();
        assert!(load_profile(&path, SettingsHost::Linux).is_err());
        assert!(load_player_profile(&path, SettingsHost::Linux).is_err());
        assert!(save_profile(&path, &native("new"), SettingsHost::Linux).is_err());
        assert!(save_player_profile(&path, &player("new"), SettingsHost::Linux).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(dir.entries(), vec![path]);
    }
}

#[test]
fn nonregular_directory_target_remains_unchanged() {
    let dir = Directory::new();
    let path = dir.path("directory.profile");
    fs::create_dir(&path).unwrap();
    let child = path.join("foreign");
    fs::write(&child, b"foreign contents").unwrap();
    assert!(load_profile(&path, SettingsHost::Linux).is_err());
    assert!(load_player_profile(&path, SettingsHost::Linux).is_err());
    assert!(save_profile(&path, &native("new"), SettingsHost::Linux).is_err());
    assert!(save_player_profile(&path, &player("new"), SettingsHost::Linux).is_err());
    assert_eq!(fs::read(child).unwrap(), b"foreign contents");
    assert_eq!(dir.entries(), vec![path]);
}

#[cfg(unix)]
#[test]
fn regular_and_dangling_symlinks_are_refused_without_touching_foreign_referent() {
    let dir = Directory::new();
    let referent = dir.path("referent.profile");
    let bytes = encode_profile(&native("foreign"), SettingsHost::Linux).unwrap();
    fs::write(&referent, &bytes).unwrap();
    for (name, target) in [
        ("linked.profile", referent.clone()),
        ("dangling.profile", dir.path("missing")),
    ] {
        let path = dir.path(name);
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(load_profile(&path, SettingsHost::Linux).is_err());
        assert!(load_player_profile(&path, SettingsHost::Linux).is_err());
        assert!(save_profile(&path, &native("new"), SettingsHost::Linux).is_err());
        assert!(save_player_profile(&path, &player("new"), SettingsHost::Linux).is_err());
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&path).unwrap(), target);
        assert_eq!(fs::read(&referent).unwrap(), bytes);
    }
    assert_eq!(dir.entries().len(), 3);
}
