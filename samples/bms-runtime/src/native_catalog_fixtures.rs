//! Deferred owned-worker fixtures using the real scan and bounded channel gates.
use super::*;
use crate::player_chart::ScanStage;
use std::{
    fs,
    sync::{mpsc, atomic::AtomicUsize},
    time::Duration,
};

struct CatalogTree(PathBuf);
impl CatalogTree {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-native-catalog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("second.bms"),
            "#TITLE ÉTOILE\n#ARTIST 作曲家\n#BPM 60\n#WAV01 absent.wav\n#00011:01\n",
        )
        .unwrap();
        fs::write(path.join("first.bms"), "#TITLE Alpha\n#BPM 60\n").unwrap();
        Self(path)
    }
}
impl Drop for CatalogTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
const WAIT: Duration = Duration::from_secs(5);

#[test]
fn actual_catalog_worker_coalesces_progress_and_publishes_only_one_joined_prepared_result() {
    let root = CatalogTree::new();
    let caller = thread::current().id();
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let mut job = NativeCatalog::spawn(root.0.clone(), move |library, control| {
        control.checkpoint()?;
        entered.send(thread::current().id()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        control.checkpoint()?;
        Ok(library)
    })
    .unwrap();
    let worker = ready.recv_timeout(WAIT).unwrap();
    assert_ne!(worker, caller);
    assert!(!job.is_finished());
    assert!(job.poll().is_none());
    let latest = job.progress();
    assert_eq!(latest.stage, ScanStage::Complete);
    assert_eq!(latest.charts, 2);
    assert_eq!(latest.directories, 1);
    for _ in 0..32 {
        assert_eq!(job.progress(), latest);
        assert!(job.poll().is_none());
    }
    // UI observation must not wait for the actual shared progress-slot writer.
    // No fixture writes invented progress or constructs a completed thread handle.
    let progress_slot = job.control.progress.clone();
    let held = progress_slot.lock().unwrap();
    assert_eq!(job.progress(), latest);
    assert!(job.poll().is_none());
    drop(held);
    release.send(()).unwrap();
    let result = job.join().unwrap().unwrap();
    assert_eq!(
        result
            .entries
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "ÉTOILE"]
    );
    assert_eq!(result.entries[1].artist, "作曲家");
    assert!(result.diagnostics.is_empty());
    assert!(job.is_finished());
    assert!(job.poll().is_none());
    assert!(job.join().is_none());
    assert_eq!(job.progress(), latest);
    drop(job);
    assert_eq!(result.entries[0].path, root.0.join("first.bms"));
}

#[test]
fn cancellation_errors_and_drop_release_the_actual_worker_without_a_late_success_or_repeated_outcome()
 {
    let root = CatalogTree::new();
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let discarded = Arc::new(AtomicUsize::new(0));
    struct PreparedDrop(Arc<AtomicUsize>);
    impl Drop for PreparedDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let tail = discarded.clone();
    let mut cancelled = NativeCatalog::spawn(root.0.clone(), move |library, control| {
        assert_eq!(library.entries.len(), 2);
        entered.send(()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        assert!(control.is_cancelled());
        // Returning a value after cancellation must still lose to the owner's
        // final cancellation check; no partial catalog becomes public.
        Ok(PreparedDrop(tail))
    })
    .unwrap();
    ready.recv_timeout(WAIT).unwrap();
    cancelled.cancel();
    cancelled.cancel();
    assert!(cancelled.poll().is_none());
    release.send(()).unwrap();
    let outcome = cancelled.join().unwrap();
    assert!(outcome.is_err());
    assert!(outcome.err().unwrap().contains("cancel"));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    assert!(cancelled.poll().is_none());
    assert!(cancelled.join().is_none());

    let prepare_calls = Arc::new(AtomicUsize::new(0));
    let calls = prepare_calls.clone();
    let mut missing = NativeCatalog::<()>::spawn(root.0.join("absent-directory"), move |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    assert!(missing.join().unwrap().is_err());
    assert_eq!(prepare_calls.load(Ordering::SeqCst), 0);
    let mut refused = NativeCatalog::<()>::spawn(root.0.clone(), |_, control| {
        control.checkpoint()?;
        Err("supplied font preparation refused".into())
    })
    .unwrap();
    assert_eq!(
        refused.join().unwrap().unwrap_err(),
        "supplied font preparation refused"
    );
    assert!(refused.poll().is_none());
    let mut panicked = NativeCatalog::<()>::spawn(root.0.clone(), |_, _| {
        panic!("controlled preparation panic")
    })
    .unwrap();
    assert!(panicked.join().unwrap().unwrap_err().contains("panicked"));
    assert!(panicked.poll().is_none());

    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let (finished, tail) = mpsc::sync_channel(1);
    let owner = NativeCatalog::spawn(root.0.clone(), move |library, _| {
        entered.send(()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        finished.send(()).unwrap();
        Ok(library)
    })
    .unwrap();
    ready.recv_timeout(WAIT).unwrap();
    assert!(!owner.is_finished());
    let control = owner.control.clone();
    release.send(()).unwrap();
    drop(owner); // Real Drop cancels and joins; no separate detached task survives it.
    assert!(control.is_cancelled());
    tail.recv_timeout(WAIT).unwrap();
    assert!(control.checkpoint().is_err());
}
