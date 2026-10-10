use super::*;
use crate::native_publication::PublicationWriter;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Barrier,
};
use std::{io::Write, path::PathBuf};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..32 {
            let path = std::env::temp_dir().join(format!(
                "beatkernel-profile-publication-{}-{}",
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
    fn path(&self) -> PathBuf {
        self.0.join("player.profile")
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
fn encoded(device: &str) -> Vec<u8> {
    encode_profile(
        &NativeSettings::from_args(&["--alsa".into(), device.into()], SettingsHost::Linux).unwrap(),
        SettingsHost::Linux,
    )
    .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    ShortInterrupted,
    Zero,
    Partial,
    Flush,
    Sync,
}
struct Writer<'a> {
    file: File,
    fault: Fault,
    writes: usize,
    closed: Arc<AtomicBool>,
    on_sync: Option<Box<dyn FnOnce() + 'a>>,
}
impl Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        match self.fault {
            Fault::ShortInterrupted if self.writes == 1 => {
                Err(io::Error::new(io::ErrorKind::Interrupted, "retry write"))
            }
            Fault::Zero => Ok(0),
            Fault::Partial if self.writes > 1 => Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "profile partial capacity",
            )),
            Fault::Partial | Fault::ShortInterrupted => {
                self.file.write(&bytes[..bytes.len().min(3)])
            }
            _ => self.file.write(bytes),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if matches!(self.fault, Fault::Flush) {
            Err(io::Error::other("profile flush failure"))
        } else {
            self.file.flush()
        }
    }
}
impl PublicationWriter for Writer<'_> {
    fn sync_all(&mut self) -> io::Result<()> {
        if matches!(self.fault, Fault::Sync) {
            return Err(io::Error::other("profile sync failure"));
        }
        self.file.sync_all()?;
        if let Some(action) = self.on_sync.take() {
            action();
        }
        Ok(())
    }
}
impl Drop for Writer<'_> {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[test]
fn short_and_interrupted_writes_publish_complete_reloadable_profiles() {
    for replacing in [false, true] {
        let dir = Directory::new();
        let path = dir.path();
        if replacing {
            fs::write(&path, encoded("old")).unwrap();
        }
        let bytes = encoded("new");
        let closed = Arc::new(AtomicBool::new(false));
        save_encoded_with(
            &path,
            &bytes,
            SettingsHost::Linux,
            validate_native_profile,
            |file| Writer {
                file,
                fault: Fault::ShortInterrupted,
                writes: 0,
                closed: closed.clone(),
                on_sync: None,
            },
            |stage| {
                assert!(closed.load(Ordering::SeqCst));
                fs::remove_file(stage)
            },
        )
        .unwrap();
        assert!(closed.load(Ordering::SeqCst));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            encode_profile(
                &load_profile(&path, SettingsHost::Linux).unwrap(),
                SettingsHost::Linux
            )
            .unwrap(),
            bytes
        );
        assert_eq!(dir.entries(), vec![path]);
    }
}

#[test]
fn writer_failures_preserve_absent_or_old_targets_and_primary_errors() {
    for replacing in [false, true] {
        for (fault, kind, payload) in [
            (Fault::Zero, io::ErrorKind::WriteZero, None),
            (
                Fault::Partial,
                io::ErrorKind::StorageFull,
                Some("profile partial capacity"),
            ),
            (
                Fault::Flush,
                io::ErrorKind::Other,
                Some("profile flush failure"),
            ),
            (
                Fault::Sync,
                io::ErrorKind::Other,
                Some("profile sync failure"),
            ),
        ] {
            let dir = Directory::new();
            let path = dir.path();
            let old = encoded("old");
            if replacing {
                fs::write(&path, &old).unwrap();
            }
            let closed = Arc::new(AtomicBool::new(false));
            let mut cleanup_calls = 0;
            let error = save_encoded_with(
                &path,
                &encoded("new"),
                SettingsHost::Linux,
                validate_native_profile,
                |file| Writer {
                    file,
                    fault,
                    writes: 0,
                    closed: closed.clone(),
                    on_sync: None,
                },
                |stage| {
                    assert!(closed.load(Ordering::SeqCst));
                    cleanup_calls += 1;
                    fs::remove_file(stage)
                },
            )
            .unwrap_err();
            let error = error.downcast_ref::<io::Error>().unwrap();
            assert_eq!(error.kind(), kind, "{fault:?}");
            if let Some(payload) = payload {
                assert_eq!(error.to_string(), payload);
            }
            assert_eq!(cleanup_calls, 1);
            assert_eq!(
                dir.entries(),
                if replacing {
                    vec![path.clone()]
                } else {
                    vec![]
                }
            );
            if replacing {
                assert_eq!(fs::read(path).unwrap(), old);
            }
        }
    }
}

#[test]
fn refused_cleanup_cannot_mask_primary_write_failure() {
    let dir = Directory::new();
    let path = dir.path();
    let closed = Arc::new(AtomicBool::new(false));
    let error = save_encoded_with(
        &path,
        &encoded("new"),
        SettingsHost::Linux,
        validate_native_profile,
        |file| Writer {
            file,
            fault: Fault::Partial,
            writes: 0,
            closed: closed.clone(),
            on_sync: None,
        },
        |_| {
            assert!(closed.load(Ordering::SeqCst));
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cleanup refused",
            ))
        },
    )
    .unwrap_err();
    let error = error.downcast_ref::<io::Error>().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::StorageFull);
    assert_eq!(error.to_string(), "profile partial capacity");
    assert!(!path.exists());
    assert_eq!(dir.entries().len(), 1);
    assert_eq!(fs::read(&dir.entries()[0]).unwrap().len(), 3);
}

#[test]
fn successful_new_link_remains_success_when_stage_cleanup_is_refused() {
    let dir = Directory::new();
    let path = dir.path();
    let bytes = encoded("new");
    let closed = Arc::new(AtomicBool::new(false));
    save_encoded_with(
        &path,
        &bytes,
        SettingsHost::Linux,
        validate_native_profile,
        |file| Writer {
            file,
            fault: Fault::ShortInterrupted,
            writes: 0,
            closed: closed.clone(),
            on_sync: None,
        },
        |_| {
            assert!(closed.load(Ordering::SeqCst));
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cleanup refused",
            ))
        },
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(load_profile(&path, SettingsHost::Linux).is_ok());
    assert_eq!(dir.entries().len(), 2);
    for entry in dir.entries() {
        assert_eq!(fs::read(entry).unwrap(), bytes);
    }
}

#[test]
fn two_actually_classified_new_saves_have_exactly_one_complete_winner() {
    let dir = Directory::new();
    let path = dir.path();
    let barrier = Arc::new(Barrier::new(2));
    let bytes = [encoded("first"), encoded("second")];
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = bytes
            .iter()
            .map(|bytes| {
                let barrier = barrier.clone();
                let path = &path;
                scope.spawn(move || {
                    save_encoded_with(
                        path,
                        bytes,
                        SettingsHost::Linux,
                        validate_native_profile,
                        |file| {
                            barrier.wait();
                            file
                        },
                        |stage| fs::remove_file(stage),
                    )
                    .map_err(|error| error.downcast::<io::Error>().unwrap())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let loser = results
        .iter()
        .find_map(|result| result.as_ref().err())
        .unwrap();
    assert_eq!(loser.kind(), io::ErrorKind::AlreadyExists);
    let winner = results.iter().position(Result::is_ok).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes[winner]);
    assert!(load_profile(&path, SettingsHost::Linux).is_ok());
    assert_eq!(dir.entries(), vec![path]);
}

#[test]
fn target_appearing_after_absent_classification_is_preserved() {
    let dir = Directory::new();
    let path = dir.path();
    let foreign = b"foreign appeared target";
    let error = save_encoded_with(
        &path,
        &encoded("new"),
        SettingsHost::Linux,
        validate_native_profile,
        |file| {
            fs::write(&path, foreign).unwrap();
            file
        },
        |stage| fs::remove_file(stage),
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(&path).unwrap(), foreign);
    assert_eq!(dir.entries(), vec![path]);
}

#[test]
fn replacement_revalidates_changed_malformed_or_foreign_target() {
    for changed in [
        b"malformed".to_vec(),
        b"BEATKERNEL-NATIVE-PROFILE\t1\twindows\n".to_vec(),
    ] {
        let dir = Directory::new();
        let path = dir.path();
        fs::write(&path, encoded("old")).unwrap();
        let closed = Arc::new(AtomicBool::new(false));
        let error = save_encoded_with(
            &path,
            &encoded("new"),
            SettingsHost::Linux,
            validate_native_profile,
            |file| Writer {
                file,
                fault: Fault::ShortInterrupted,
                writes: 0,
                closed: closed.clone(),
                on_sync: Some(Box::new(|| {
                    fs::write(&path, &changed).unwrap();
                })),
            },
            |stage| {
                assert!(closed.load(Ordering::SeqCst));
                fs::remove_file(stage)
            },
        )
        .unwrap_err();
        assert!(!error.to_string().is_empty());
        assert_eq!(fs::read(&path).unwrap(), changed);
        assert_eq!(dir.entries(), vec![path]);
    }
}

#[cfg(unix)]
#[test]
fn rename_refusal_after_valid_revalidation_preserves_old_profile() {
    let dir = Directory::new();
    let path = dir.path();
    let old = encoded("old");
    fs::write(&path, &old).unwrap();
    let closed = Arc::new(AtomicBool::new(false));
    let mut expected = None;
    let error = save_encoded_with(
        &path,
        &encoded("new"),
        SettingsHost::Linux,
        validate_native_profile,
        |file| Writer {
            file,
            fault: Fault::ShortInterrupted,
            writes: 0,
            closed: closed.clone(),
            on_sync: Some(Box::new(|| {
                let stage = dir
                    .entries()
                    .into_iter()
                    .find(|entry| entry != &path)
                    .unwrap();
                fs::remove_file(&stage).unwrap();
                expected = Some(fs::rename(&stage, &path).unwrap_err());
            })),
        },
        |stage| {
            assert!(closed.load(Ordering::SeqCst));
            fs::remove_file(stage)
        },
    )
    .unwrap_err();
    let error = error.downcast_ref::<io::Error>().unwrap();
    let expected = expected.unwrap();
    assert_eq!(error.kind(), expected.kind());
    assert_eq!(error.raw_os_error(), expected.raw_os_error());
    assert_eq!(error.to_string(), expected.to_string());
    assert_eq!(fs::read(&path).unwrap(), old);
    assert_eq!(dir.entries(), vec![path]);
}

#[test]
fn native_replacement_refuses_target_changed_to_valid_v2() {
    let dir = Directory::new();
    let path = dir.path();
    fs::write(&path, encoded("old")).unwrap();
    let model = PlayerProfile {
        native: NativeSettings::from_args(&[], SettingsHost::Linux).unwrap(),
        presentation: crate::presentation_settings::PresentationSettings::default(),
    };
    let changed = encode_player_profile(&model, SettingsHost::Linux).unwrap();
    let expected = validate_native_profile(&changed, SettingsHost::Linux).unwrap_err();
    let closed = Arc::new(AtomicBool::new(false));
    let error = save_encoded_with(
        &path,
        &encoded("new"),
        SettingsHost::Linux,
        validate_native_profile,
        |file| Writer {
            file,
            fault: Fault::ShortInterrupted,
            writes: 0,
            closed: closed.clone(),
            on_sync: Some(Box::new(|| {
                fs::write(&path, &changed).unwrap();
            })),
        },
        |stage| {
            assert!(closed.load(Ordering::SeqCst));
            fs::remove_file(stage)
        },
    )
    .unwrap_err();
    assert_eq!(error.to_string(), expected);
    assert_eq!(fs::read(&path).unwrap(), changed);
    assert_eq!(dir.entries(), vec![path]);
}

#[cfg(unix)]
#[test]
fn replacement_revalidation_refuses_changed_symlink_without_touching_referent() {
    let dir = Directory::new();
    let path = dir.path();
    let referent = dir.0.join("referent");
    fs::write(&path, encoded("old")).unwrap();
    fs::write(&referent, b"foreign referent").unwrap();
    let closed = Arc::new(AtomicBool::new(false));
    save_encoded_with(
        &path,
        &encoded("new"),
        SettingsHost::Linux,
        validate_native_profile,
        |file| Writer {
            file,
            fault: Fault::ShortInterrupted,
            writes: 0,
            closed: closed.clone(),
            on_sync: Some(Box::new(|| {
                fs::remove_file(&path).unwrap();
                std::os::unix::fs::symlink(&referent, &path).unwrap();
            })),
        },
        |stage| {
            assert!(closed.load(Ordering::SeqCst));
            fs::remove_file(stage)
        },
    )
    .unwrap_err();
    assert!(fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&referent).unwrap(), b"foreign referent");
    assert_eq!(dir.entries().len(), 2);
}

#[test]
fn initial_validation_error_occurs_before_writer_or_cleanup_and_is_preserved() {
    let dir = Directory::new();
    let path = dir.path();
    let invalid = b"malformed target";
    fs::write(&path, invalid).unwrap();
    let expected = validate_native_profile(invalid, SettingsHost::Linux).unwrap_err();
    let error = save_encoded_with(
        &path,
        &encoded("new"),
        SettingsHost::Linux,
        validate_native_profile,
        |_: File| -> File { panic!("writer called") },
        |_| -> io::Result<()> { panic!("cleanup called") },
    )
    .unwrap_err();
    assert_eq!(error.to_string(), expected);
    assert_eq!(fs::read(&path).unwrap(), invalid);
    assert_eq!(dir.entries(), vec![path]);
}
