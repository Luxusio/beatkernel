use super::{
    publish_new, publish_new_with, publish_new_with_candidates,
    publish_new_with_candidates_and_cleanup, publish_with_candidates_and_cleanup,
    CommitDisposition, PublicationWriter,
};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-publication-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn names(&self) -> Vec<PathBuf> {
        let mut names: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn publishes_exact_bytes_and_cleans_owned_stage() {
    let dir = TempDir::new();
    let final_path = dir.path("record.bkr");
    let bytes = b"\0BKRESULT\xff\r\ncomplete bytes";
    publish_new(&final_path, bytes).unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), bytes);
    assert_eq!(dir.names(), vec![final_path]);
}

#[test]
fn publishes_empty_file() {
    let dir = TempDir::new();
    let final_path = dir.path("empty");
    publish_new(&final_path, b"").unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), b"");
    assert_eq!(dir.names(), vec![final_path]);
}

#[test]
fn staging_name_is_ascii_and_independent_of_target_basename() {
    let dir = TempDir::new();
    let basename = "長".repeat(70);
    let final_path = dir.path(&basename);
    publish_new_with(&final_path, b"complete", |file| {
        let stages = dir.names();
        assert_eq!(stages.len(), 1);
        let stage_name = stages[0].file_name().unwrap().to_str().unwrap();
        assert!(stage_name.is_ascii());
        let (stem, extension) = stage_name.split_once('.').unwrap();
        assert_eq!(stem.len(), 8);
        assert_eq!(extension.len(), 3);
        assert!(stem
            .bytes()
            .chain(extension.bytes())
            .all(|byte| { byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte) }));
        assert!(!stage_name.contains(&basename));
        assert_ne!(stages[0], final_path);
        file
    })
    .unwrap();
    assert_eq!(fs::read(&final_path).unwrap(), b"complete");
    assert_eq!(dir.names(), vec![final_path]);
}

#[test]
fn existing_regular_file_is_preserved() {
    let dir = TempDir::new();
    let final_path = dir.path("existing");
    fs::write(&final_path, b"foreign original").unwrap();
    let expected = fs::hard_link(&final_path, &final_path).unwrap_err();
    let error = publish_new(&final_path, b"new complete value").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(error.raw_os_error(), expected.raw_os_error());
    assert_eq!(error.to_string(), expected.to_string());
    assert_eq!(fs::read(&final_path).unwrap(), b"foreign original");
    assert_eq!(dir.names(), vec![final_path]);
}

#[cfg(unix)]
#[test]
fn dangling_symlink_is_preserved() {
    let dir = TempDir::new();
    let final_path = dir.path("existing-link");
    let target = dir.path("absent-target");
    std::os::unix::fs::symlink(&target, &final_path).unwrap();
    let error = publish_new(&final_path, b"new complete value").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert!(fs::symlink_metadata(&final_path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(&final_path).unwrap(), target);
    assert!(!target.exists());
    assert_eq!(dir.names(), vec![final_path]);
}

struct BarrierWriter {
    file: File,
    barrier: Arc<Barrier>,
}

impl Write for BarrierWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl PublicationWriter for BarrierWriter {
    fn sync_all(&mut self) -> io::Result<()> {
        self.file.sync_all()?;
        self.barrier.wait();
        Ok(())
    }
}

#[test]
fn concurrent_publishers_have_one_complete_winner_and_one_exclusive_refusal() {
    let dir = TempDir::new();
    let final_path = dir.path("race");
    let barrier = Arc::new(Barrier::new(2));
    let payloads = [vec![0x31; 8193], vec![0xd7; 12289]];
    std::thread::scope(|scope| {
        let handles: Vec<_> = payloads
            .iter()
            .map(|bytes| {
                let barrier = Arc::clone(&barrier);
                let final_path = &final_path;
                scope.spawn(move || {
                    publish_new_with(final_path, bytes, |file| BarrierWriter { file, barrier })
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let loser = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .unwrap();
        assert_eq!(loser.kind(), io::ErrorKind::AlreadyExists);
        let winner = results.iter().position(|result| result.is_ok()).unwrap();
        assert_eq!(fs::read(&final_path).unwrap(), payloads[winner]);
    });
    assert_eq!(dir.names(), vec![final_path]);
}

#[test]
fn invalid_parent_and_nul_path_leave_no_final_or_stage() {
    let dir = TempDir::new();
    let missing = dir.path("missing").join("result");
    assert_eq!(
        publish_new(&missing, b"bytes").unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    let file_parent = dir.path("file-parent");
    fs::write(&file_parent, b"parent original").unwrap();
    assert!(publish_new(&file_parent.join("result"), b"bytes").is_err());
    assert_eq!(fs::read(&file_parent).unwrap(), b"parent original");
    assert!(publish_new(&dir.path("invalid\0name"), b"bytes").is_err());
    assert!(!missing.exists());
    assert_eq!(dir.names(), vec![file_parent]);
}

#[test]
fn foreign_stage_collision_is_preserved_then_next_candidate_publishes() {
    let dir = TempDir::new();
    let final_path = dir.path("result");
    let foreign = dir.path("foreign-stage");
    let owned = dir.path("owned-stage");
    fs::write(&foreign, b"foreign original").unwrap();
    let mut attempts = 0;
    publish_new_with_candidates(
        &final_path,
        b"complete result",
        |file| file,
        || {
            attempts += 1;
            Ok(if attempts == 1 {
                foreign.clone()
            } else {
                owned.clone()
            })
        },
    )
    .unwrap();
    assert_eq!(attempts, 2);
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign original");
    assert_eq!(fs::read(&final_path).unwrap(), b"complete result");
    assert!(!owned.exists());
    let mut expected = vec![foreign, final_path];
    expected.sort();
    assert_eq!(dir.names(), expected);
}

#[test]
fn stage_collision_retries_are_bounded_and_return_original_create_error() {
    let dir = TempDir::new();
    let final_path = dir.path("result");
    let foreign = dir.path("foreign-stage");
    fs::write(&foreign, b"foreign original").unwrap();
    let expected = File::options()
        .write(true)
        .create_new(true)
        .open(&foreign)
        .unwrap_err();
    let mut attempts = 0;
    let error = publish_new_with_candidates(
        &final_path,
        b"bytes",
        |file| file,
        || {
            attempts += 1;
            Ok(foreign.clone())
        },
    )
    .unwrap_err();
    assert_eq!(attempts, 32);
    assert_eq!(error.kind(), expected.kind());
    assert_eq!(error.raw_os_error(), expected.raw_os_error());
    assert_eq!(error.to_string(), expected.to_string());
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign original");
    assert!(!final_path.exists());
    assert_eq!(dir.names(), vec![foreign]);
}

#[test]
fn candidate_equal_to_final_is_refused_before_creating_or_wrapping() {
    let dir = TempDir::new();
    let final_path = dir.path("result");
    let mut wrapped = false;
    let error = publish_new_with_candidates(
        &final_path,
        b"bytes",
        |file| {
            wrapped = true;
            file
        },
        || Ok(final_path.clone()),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(!wrapped);
    assert!(dir.names().is_empty());
}

#[test]
fn candidate_and_final_native_filename_aliases_are_refused_without_effects() {
    let dir = TempDir::new();
    let plain = "1234ABCD.0EF";
    let aliases = [
        "1234abcd.0ef",
        "1234ABCD.0EF.",
        "1234ABCD.0EF ",
        " 1234ABCD.0EF",
        "  1234abcd.0ef.  ",
        "1234ABCD.0EF::$DATA",
        "1234ABCD.0EF:stream:$DATA",
        " 1234abcd.0ef. :stream:$DATA",
    ];
    for alias in aliases {
        for (stage_name, final_name) in [(plain, alias), (alias, plain)] {
            let stage = dir.path(stage_name);
            let final_path = dir.path(final_name);
            let mut wrapped = false;
            let error = publish_new_with_candidates(
                &final_path,
                b"complete result",
                |file| {
                    wrapped = true;
                    file
                },
                || Ok(stage.clone()),
            )
            .unwrap_err();
            assert_eq!(
                error.kind(),
                io::ErrorKind::InvalidInput,
                "{stage_name:?} -> {final_name:?}"
            );
            assert!(
                !wrapped,
                "alias acquired writer: {stage_name:?} -> {final_name:?}"
            );
            assert!(
                dir.names().is_empty(),
                "alias created a file: {stage_name:?} -> {final_name:?}"
            );
        }
    }
}

#[test]
fn candidate_generation_refusal_preserves_exact_error_without_filesystem_effects() {
    let dir = TempDir::new();
    let mut wrapped = false;
    let error = publish_new_with_candidates(
        &dir.path("result"),
        b"bytes",
        |file| {
            wrapped = true;
            file
        },
        || {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "fixture candidate identity exhausted",
            ))
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "fixture candidate identity exhausted");
    assert!(!wrapped);
    assert!(dir.names().is_empty());
}

#[derive(Clone, Copy)]
enum Fault {
    ShortInterrupted,
    Zero,
    PartialCapacity,
    Flush,
    Sync,
}

#[derive(Default)]
struct Observation {
    bytes_at_close: Vec<u8>,
    stage_present_at_close: bool,
    final_present_at_close: bool,
    closed: bool,
    writes: usize,
}

struct FaultWriter {
    file: Option<File>,
    stage: PathBuf,
    final_path: PathBuf,
    fault: Fault,
    written: usize,
    calls: usize,
    observed: Arc<Mutex<Observation>>,
}

impl Write for FaultWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        match self.fault {
            Fault::ShortInterrupted if self.calls == 1 => {
                return Err(io::Error::from(io::ErrorKind::Interrupted))
            }
            Fault::Zero => return Ok(0),
            Fault::PartialCapacity if self.written >= 3 => {
                return Err(io::Error::from_raw_os_error(28))
            }
            _ => {}
        }
        let limit = match self.fault {
            Fault::ShortInterrupted => 2,
            Fault::PartialCapacity => 3 - self.written,
            _ => bytes.len(),
        };
        let count = self
            .file
            .as_mut()
            .unwrap()
            .write(&bytes[..bytes.len().min(limit)])?;
        self.written += count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if matches!(self.fault, Fault::Flush) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture flush refusal",
            ));
        }
        self.file.as_mut().unwrap().flush()
    }
}

impl PublicationWriter for FaultWriter {
    fn sync_all(&mut self) -> io::Result<()> {
        if matches!(self.fault, Fault::Sync) {
            return Err(io::Error::new(io::ErrorKind::Other, "fixture sync refusal"));
        }
        self.file.as_mut().unwrap().sync_all()
    }
}

impl Drop for FaultWriter {
    fn drop(&mut self) {
        drop(self.file.take());
        let mut observed = self.observed.lock().unwrap();
        observed.closed = self.file.is_none();
        observed.stage_present_at_close = self.stage.exists();
        observed.final_present_at_close = self.final_path.exists();
        observed.bytes_at_close = fs::read(&self.stage).unwrap();
        observed.writes = self.calls;
    }
}

fn run_fault(dir: &TempDir, fault: Fault) -> (io::Result<()>, Arc<Mutex<Observation>>) {
    let stage = dir.path("controlled-stage");
    let final_path = dir.path("result");
    let observed = Arc::new(Mutex::new(Observation::default()));
    let result = publish_new_with_candidates(
        &final_path,
        b"abcdefgh",
        |file| FaultWriter {
            file: Some(file),
            stage: stage.clone(),
            final_path: final_path.clone(),
            fault,
            written: 0,
            calls: 0,
            observed: Arc::clone(&observed),
        },
        || Ok(stage.clone()),
    );
    (result, observed)
}

fn assert_closed_before_cleanup(observed: &Observation) {
    assert!(observed.closed);
    assert!(
        observed.stage_present_at_close,
        "owned stage removed before handle close"
    );
    assert!(
        !observed.final_present_at_close,
        "final published before handle close"
    );
}

#[test]
fn production_write_all_retries_interrupted_and_short_real_file_writes() {
    let dir = TempDir::new();
    let (result, observed) = run_fault(&dir, Fault::ShortInterrupted);
    result.unwrap();
    let observed = observed.lock().unwrap();
    assert_closed_before_cleanup(&observed);
    assert_eq!(observed.writes, 5);
    assert_eq!(observed.bytes_at_close, b"abcdefgh");
    assert_eq!(fs::read(dir.path("result")).unwrap(), b"abcdefgh");
    assert_eq!(dir.names(), vec![dir.path("result")]);
}

#[test]
fn zero_write_returns_write_zero_without_final_and_cleans_stage() {
    let dir = TempDir::new();
    let (result, observed) = run_fault(&dir, Fault::Zero);
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WriteZero);
    let observed = observed.lock().unwrap();
    assert_closed_before_cleanup(&observed);
    assert_eq!(observed.bytes_at_close, b"");
    assert!(dir.names().is_empty());
}

#[test]
fn partial_capacity_refusal_returns_exact_error_without_final_and_cleans_stage() {
    let dir = TempDir::new();
    let (result, observed) = run_fault(&dir, Fault::PartialCapacity);
    let error = result.unwrap_err();
    let expected = io::Error::from_raw_os_error(28);
    assert_eq!(error.raw_os_error(), Some(28));
    assert_eq!(error.kind(), expected.kind());
    assert_eq!(error.to_string(), expected.to_string());
    let observed = observed.lock().unwrap();
    assert_closed_before_cleanup(&observed);
    assert_eq!(observed.bytes_at_close, b"abc");
    assert!(dir.names().is_empty());
}

#[test]
fn flush_refusal_returns_exact_error_without_final_and_cleans_stage() {
    let dir = TempDir::new();
    let (result, observed) = run_fault(&dir, Fault::Flush);
    let error = result.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "fixture flush refusal");
    let observed = observed.lock().unwrap();
    assert_closed_before_cleanup(&observed);
    assert_eq!(observed.bytes_at_close, b"abcdefgh");
    assert!(dir.names().is_empty());
}

#[test]
fn sync_refusal_returns_exact_error_without_final_and_cleans_stage() {
    let dir = TempDir::new();
    let (result, observed) = run_fault(&dir, Fault::Sync);
    let error = result.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "fixture sync refusal");
    let observed = observed.lock().unwrap();
    assert_closed_before_cleanup(&observed);
    assert_eq!(observed.bytes_at_close, b"abcdefgh");
    assert!(dir.names().is_empty());
}

#[test]
fn refused_cleanup_preserves_exact_primary_write_error_and_never_creates_final() {
    let dir = TempDir::new();
    let stage = dir.path("controlled-stage");
    let final_path = dir.path("result");
    let observed = Arc::new(Mutex::new(Observation::default()));
    let mut cleanup_calls = 0;
    let result = publish_new_with_candidates_and_cleanup(
        &final_path,
        b"abcdefgh",
        |file| FaultWriter {
            file: Some(file),
            stage: stage.clone(),
            final_path: final_path.clone(),
            fault: Fault::PartialCapacity,
            written: 0,
            calls: 0,
            observed: Arc::clone(&observed),
        },
        || Ok(stage.clone()),
        |owned_stage| {
            cleanup_calls += 1;
            assert_eq!(owned_stage, stage);
            assert_closed_before_cleanup(&observed.lock().unwrap());
            assert_eq!(fs::read(owned_stage).unwrap(), b"abc");
            assert!(!final_path.exists());
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture cleanup refusal",
            ))
        },
    );
    let error = result.unwrap_err();
    assert_eq!(cleanup_calls, 1);
    assert_eq!(error.raw_os_error(), Some(28));
    assert_eq!(
        error.to_string(),
        io::Error::from_raw_os_error(28).to_string()
    );
    assert_closed_before_cleanup(&observed.lock().unwrap());
    assert!(!final_path.exists());
    assert_eq!(fs::read(&stage).unwrap(), b"abc");
    assert_eq!(dir.names(), vec![stage]);
}

#[test]
fn refused_cleanup_after_commit_preserves_success_and_complete_final() {
    let dir = TempDir::new();
    let stage = dir.path("controlled-stage");
    let final_path = dir.path("result");
    let observed = Arc::new(Mutex::new(Observation::default()));
    let mut cleanup_calls = 0;
    publish_new_with_candidates_and_cleanup(
        &final_path,
        b"abcdefgh",
        |file| FaultWriter {
            file: Some(file),
            stage: stage.clone(),
            final_path: final_path.clone(),
            fault: Fault::ShortInterrupted,
            written: 0,
            calls: 0,
            observed: Arc::clone(&observed),
        },
        || Ok(stage.clone()),
        |owned_stage| {
            cleanup_calls += 1;
            assert_eq!(owned_stage, stage);
            assert_closed_before_cleanup(&observed.lock().unwrap());
            assert_eq!(fs::read(owned_stage).unwrap(), b"abcdefgh");
            assert_eq!(fs::read(&final_path).unwrap(), b"abcdefgh");
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture cleanup refusal",
            ))
        },
    )
    .unwrap();
    assert_eq!(cleanup_calls, 1);
    assert_eq!(fs::read(&final_path).unwrap(), b"abcdefgh");
    assert_eq!(fs::read(&stage).unwrap(), b"abcdefgh");
    let mut expected = vec![stage, final_path];
    expected.sort();
    assert_eq!(dir.names(), expected);
}

#[test]
fn refused_cleanup_preserves_original_link_error_and_existing_final() {
    let dir = TempDir::new();
    let stage = dir.path("controlled-stage");
    let final_path = dir.path("result");
    fs::write(&final_path, b"foreign original").unwrap();
    let expected = fs::hard_link(&final_path, &final_path).unwrap_err();
    let mut cleanup_calls = 0;
    let error = publish_new_with_candidates_and_cleanup(
        &final_path,
        b"new complete value",
        |file| file,
        || Ok(stage.clone()),
        |owned_stage| {
            cleanup_calls += 1;
            assert_eq!(owned_stage, stage);
            assert_eq!(fs::read(owned_stage).unwrap(), b"new complete value");
            assert_eq!(fs::read(&final_path).unwrap(), b"foreign original");
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture cleanup refusal",
            ))
        },
    )
    .unwrap_err();
    assert_eq!(cleanup_calls, 1);
    assert_eq!(error.kind(), expected.kind());
    assert_eq!(error.raw_os_error(), expected.raw_os_error());
    assert_eq!(error.to_string(), expected.to_string());
    assert_eq!(fs::read(&final_path).unwrap(), b"foreign original");
    assert_eq!(fs::read(&stage).unwrap(), b"new complete value");
}

#[test]
fn failed_stage_create_never_claims_cleanup_ownership() {
    let dir = TempDir::new();
    let foreign = dir.path("foreign-stage");
    fs::write(&foreign, b"foreign original").unwrap();
    let mut cleanup_calls = 0;
    let error = publish_new_with_candidates_and_cleanup(
        &dir.path("result"),
        b"bytes",
        |file| file,
        || Ok(foreign.clone()),
        |_| {
            cleanup_calls += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(cleanup_calls, 0);
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign original");
    assert_eq!(dir.names(), vec![foreign]);
}

#[test]
fn reserved_final_names_and_native_aliases_are_rejected_before_any_effect() {
    let dir = TempDir::new();
    let aliases = [
        "1234ABCD.0EF",
        "1234abcd.0ef",
        "1234ABCD.0EF.",
        "1234ABCD.0EF ",
        " 1234ABCD.0EF",
        " 1234abcd.0ef.  ",
        "1234ABCD.0EF::$DATA",
        "1234ABCD.0EF:stream:$DATA",
    ];
    for existing in [false, true] {
        let foreign = dir.path("1234ABCD.0EF");
        if existing {
            fs::write(&foreign, b"foreign original").unwrap();
        }
        for alias in aliases {
            let mut candidate_calls = 0;
            let mut wrap_calls = 0;
            let mut cleanup_calls = 0;
            let error = publish_new_with_candidates_and_cleanup(
                &dir.path(alias),
                b"new complete value",
                |file| {
                    wrap_calls += 1;
                    file
                },
                || {
                    candidate_calls += 1;
                    Ok(dir.path("controlled-stage"))
                },
                |_| {
                    cleanup_calls += 1;
                    Ok(())
                },
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{alias:?}");
            assert_eq!((candidate_calls, wrap_calls, cleanup_calls), (0, 0, 0));
            if existing {
                assert_eq!(fs::read(&foreign).unwrap(), b"foreign original");
                assert_eq!(dir.names(), vec![foreign.clone()]);
            } else {
                assert!(dir.names().is_empty());
            }
        }
    }
}

#[test]
fn another_requested_final_cannot_accept_an_incomplete_reserved_stage() {
    let dir = TempDir::new();
    let stage = dir.path("1234ABCD.0EF");
    let final_path = dir.path("first-result");
    let observed = Arc::new(Mutex::new(Observation::default()));
    let mut second_candidate_calls = 0;
    let mut second_wrap_calls = 0;
    let mut second_cleanup_calls = 0;
    let mut first_cleanup_calls = 0;
    let error = publish_new_with_candidates_and_cleanup(
        &final_path,
        b"abcdefgh",
        |file| FaultWriter {
            file: Some(file),
            stage: stage.clone(),
            final_path: final_path.clone(),
            fault: Fault::PartialCapacity,
            written: 0,
            calls: 0,
            observed: Arc::clone(&observed),
        },
        || Ok(stage.clone()),
        |owned_stage| {
            first_cleanup_calls += 1;
            assert_closed_before_cleanup(&observed.lock().unwrap());
            assert_eq!(fs::read(owned_stage).unwrap(), b"abc");
            let second_error = publish_new_with_candidates_and_cleanup(
                &stage,
                b"second complete value",
                |file| {
                    second_wrap_calls += 1;
                    file
                },
                || {
                    second_candidate_calls += 1;
                    Ok(dir.path("second-stage"))
                },
                |_| {
                    second_cleanup_calls += 1;
                    Ok(())
                },
            )
            .unwrap_err();
            assert_eq!(second_error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(fs::read(owned_stage).unwrap(), b"abc");
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture cleanup refusal",
            ))
        },
    )
    .unwrap_err();
    assert_eq!(error.raw_os_error(), Some(28));
    assert_eq!(first_cleanup_calls, 1);
    assert_eq!(
        (
            second_candidate_calls,
            second_wrap_calls,
            second_cleanup_calls
        ),
        (0, 0, 0)
    );
    assert!(!final_path.exists());
    assert_eq!(fs::read(&stage).unwrap(), b"abc");
    assert_eq!(dir.names(), vec![stage]);
}

#[test]
fn concurrent_distinct_nonreserved_finals_each_publish_complete_bytes() {
    let dir = TempDir::new();
    let paths = [dir.path("first-result"), dir.path("second-result")];
    let payloads = [vec![0x27; 8193], vec![0xa4; 12289]];
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .iter()
            .zip(&payloads)
            .map(|(path, bytes)| {
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    publish_new_with(path, bytes, |file| BarrierWriter { file, barrier })
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    });
    for (path, bytes) in paths.iter().zip(&payloads) {
        assert_eq!(fs::read(path).unwrap(), *bytes);
    }
    let mut expected = paths.to_vec();
    expected.sort();
    assert_eq!(dir.names(), expected);
}

#[test]
fn nonreserved_alias_candidates_still_refuse_before_creating_or_wrapping() {
    let dir = TempDir::new();
    let final_path = dir.path("record");
    for candidate in ["RECORD", " record", "record.", "record ", "record::$DATA"] {
        let mut wrapped = false;
        let error = publish_new_with_candidates(
            &final_path,
            b"bytes",
            |file| {
                wrapped = true;
                file
            },
            || Ok(dir.path(candidate)),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!wrapped);
        assert!(dir.names().is_empty());
    }
}

#[test]
fn commit_refusal_preserves_opaque_box_identity_despite_cleanup_refusal() {
    let dir = TempDir::new();
    let stage = dir.path("controlled-stage");
    let final_path = dir.path("result");
    fs::write(&final_path, b"foreign original").unwrap();
    let observed = Arc::new(Mutex::new(Observation::default()));
    let mut original_error_address = None;
    let mut cleanup_calls = 0;
    let error: Box<dyn std::error::Error> = publish_with_candidates_and_cleanup(
        &final_path,
        b"abcdefgh",
        |file| FaultWriter {
            file: Some(file),
            stage: stage.clone(),
            final_path: final_path.clone(),
            fault: Fault::ShortInterrupted,
            written: 0,
            calls: 0,
            observed: Arc::clone(&observed),
        },
        || Ok(stage.clone()),
        |owned_stage, target| {
            let observed = observed.lock().unwrap();
            assert!(observed.closed);
            assert!(observed.stage_present_at_close);
            assert!(observed.final_present_at_close);
            assert_eq!(observed.bytes_at_close, b"abcdefgh");
            assert_eq!(fs::read(owned_stage).unwrap(), b"abcdefgh");
            let error: Box<dyn std::error::Error> =
                Box::new(fs::hard_link(owned_stage, target).unwrap_err());
            original_error_address = Some((&*error as *const dyn std::error::Error).cast::<()>());
            Err(error)
        },
        |owned_stage| {
            cleanup_calls += 1;
            assert!(observed.lock().unwrap().closed);
            assert_eq!(fs::read(owned_stage).unwrap(), b"abcdefgh");
            assert_eq!(fs::read(&final_path).unwrap(), b"foreign original");
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture cleanup refusal",
            ))
        },
    )
    .unwrap_err();
    assert_eq!(cleanup_calls, 1);
    assert_eq!(
        Some((&*error as *const dyn std::error::Error).cast::<()>()),
        original_error_address
    );
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(&final_path).unwrap(), b"foreign original");
    assert_eq!(fs::read(&stage).unwrap(), b"abcdefgh");
    let mut expected = vec![stage, final_path];
    expected.sort();
    assert_eq!(dir.names(), expected);
}

#[test]
fn moved_commit_retires_stage_ownership_and_preserves_reused_foreign_path() {
    let dir = TempDir::new();
    let stage = dir.path("controlled-stage");
    let final_path = dir.path("result");
    fs::write(&final_path, b"old complete profile").unwrap();
    let observed = Arc::new(Mutex::new(Observation::default()));
    let mut cleanup_calls = 0;
    let result: io::Result<()> = publish_with_candidates_and_cleanup(
        &final_path,
        b"abcdefgh",
        |file| FaultWriter {
            file: Some(file),
            stage: stage.clone(),
            final_path: final_path.clone(),
            fault: Fault::ShortInterrupted,
            written: 0,
            calls: 0,
            observed: Arc::clone(&observed),
        },
        || Ok(stage.clone()),
        |owned_stage, target| {
            let observed = observed.lock().unwrap();
            assert!(observed.closed);
            assert!(observed.stage_present_at_close);
            assert!(observed.final_present_at_close);
            assert_eq!(observed.bytes_at_close, b"abcdefgh");
            assert_eq!(fs::read(target).unwrap(), b"old complete profile");
            fs::rename(owned_stage, target)?;
            assert_eq!(fs::read(target).unwrap(), b"abcdefgh");
            assert!(!owned_stage.exists());
            let mut foreign = File::options()
                .write(true)
                .create_new(true)
                .open(owned_stage)
                .unwrap();
            foreign
                .write_all(b"foreign replacement at released path")
                .unwrap();
            foreign.flush().unwrap();
            drop(foreign);
            Ok(CommitDisposition::Moved)
        },
        |owned_stage| {
            cleanup_calls += 1;
            fs::remove_file(owned_stage)
        },
    );
    result.unwrap();
    assert_eq!(cleanup_calls, 0);
    assert_eq!(fs::read(&final_path).unwrap(), b"abcdefgh");
    assert_eq!(
        fs::read(&stage).unwrap(),
        b"foreign replacement at released path"
    );
    let mut expected = vec![stage, final_path];
    expected.sort();
    assert_eq!(dir.names(), expected);
}

#[test]
fn linked_commit_observes_complete_closed_writer_then_cleans_owned_stage() {
    let dir = TempDir::new();
    let stage = dir.path("controlled-stage");
    let final_path = dir.path("result");
    let observed = Arc::new(Mutex::new(Observation::default()));
    let mut cleanup_calls = 0;
    let result: io::Result<()> = publish_with_candidates_and_cleanup(
        &final_path,
        b"abcdefgh",
        |file| FaultWriter {
            file: Some(file),
            stage: stage.clone(),
            final_path: final_path.clone(),
            fault: Fault::ShortInterrupted,
            written: 0,
            calls: 0,
            observed: Arc::clone(&observed),
        },
        || Ok(stage.clone()),
        |owned_stage, target| {
            let observed = observed.lock().unwrap();
            assert_closed_before_cleanup(&observed);
            assert_eq!(observed.bytes_at_close, b"abcdefgh");
            assert_eq!(observed.writes, 5);
            assert_eq!(fs::read(owned_stage).unwrap(), b"abcdefgh");
            fs::hard_link(owned_stage, target)?;
            Ok(CommitDisposition::Linked)
        },
        |owned_stage| {
            cleanup_calls += 1;
            assert_closed_before_cleanup(&observed.lock().unwrap());
            assert_eq!(fs::read(owned_stage).unwrap(), b"abcdefgh");
            assert_eq!(fs::read(&final_path).unwrap(), b"abcdefgh");
            fs::remove_file(owned_stage)
        },
    );
    result.unwrap();
    assert_eq!(cleanup_calls, 1);
    assert_eq!(fs::read(&final_path).unwrap(), b"abcdefgh");
    assert_eq!(dir.names(), vec![final_path]);
}
