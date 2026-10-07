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

#[test]
fn direct_preparation_runs_on_the_owned_thread_without_scan_progress_or_early_publication() {
    let original = PathBuf::from("literal-parent/../ÉTOILE 曲.bms");
    let supplied = original.clone();
    let caller = thread::current().id();
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let mut job = NativeCatalog::spawn_prepared(move |control| {
        control.checkpoint()?;
        entered.send(thread::current().id()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        control.checkpoint()?;
        // This constructor has no root to scan. Preserve arbitrary CPU-owned
        // input literally, without requiring the path or its parent to exist.
        Ok((supplied, String::from("prepared on worker")))
    })
    .unwrap();
    assert_ne!(ready.recv_timeout(WAIT).unwrap(), caller);
    let no_scan = ScanProgress {
        stage: ScanStage::Traversal,
        directories: 0,
        entries: 0,
        charts: 0,
        bytes: 0,
    };
    for _ in 0..8 {
        assert!(!job.is_finished());
        assert!(job.poll().is_none());
        assert_eq!(job.progress(), no_scan);
    }
    let progress_slot = job.control.progress.clone();
    let held = progress_slot.lock().unwrap();
    assert_eq!(job.progress(), no_scan);
    assert!(job.poll().is_none());
    drop(held);
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + WAIT;
    while !job.is_finished() {
        assert!(std::time::Instant::now() < deadline);
        thread::yield_now();
    }
    let prepared = job.poll().unwrap().unwrap();
    assert_eq!(prepared.0, original);
    assert_eq!(prepared.1, "prepared on worker");
    assert_eq!(job.progress(), no_scan);
    assert!(job.poll().is_none());
    assert!(job.join().is_none());
    drop(job);
    assert_eq!(prepared.0, PathBuf::from("literal-parent/../ÉTOILE 曲.bms"));
}

#[test]
fn direct_preparation_cancellation_errors_and_drop_join_the_same_worker_once() {
    struct Dropped(Arc<AtomicUsize>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let discarded = Arc::new(AtomicUsize::new(0));
    let value = discarded.clone();
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let mut job = NativeCatalog::spawn_prepared(move |control| {
        entered.send(()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        assert!(control.is_cancelled());
        assert!(control.checkpoint().is_err());
        Ok(Dropped(value))
    })
    .unwrap();
    ready.recv_timeout(WAIT).unwrap();
    job.cancel();
    job.cancel();
    assert!(job.poll().is_none());
    release.send(()).unwrap();
    assert!(job.join().unwrap().err().unwrap().contains("cancel"));
    assert_eq!(discarded.load(Ordering::SeqCst), 1);
    assert_eq!(job.progress(), ScanProgress::default());
    assert!(job.poll().is_none());
    assert!(job.join().is_none());

    let mut refused = NativeCatalog::<()>::spawn_prepared(|control| {
        control.checkpoint()?;
        Err("direct title font rejected".into())
    })
    .unwrap();
    assert_eq!(
        refused.join().unwrap().unwrap_err(),
        "direct title font rejected"
    );
    assert!(refused.poll().is_none());
    let mut panicked =
        NativeCatalog::<()>::spawn_prepared(|_| panic!("controlled direct preparation panic"))
            .unwrap();
    assert!(panicked.join().unwrap().unwrap_err().contains("panicked"));
    assert!(panicked.join().is_none());

    let worker_tail = Arc::new(AtomicUsize::new(0));
    let tail = Dropped(worker_tail.clone());
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let owner = NativeCatalog::spawn_prepared(move |_| {
        let _tail = tail;
        entered.send(()).unwrap();
        gate.recv_timeout(WAIT).map_err(|error| error.to_string())?;
        Ok(())
    })
    .unwrap();
    ready.recv_timeout(WAIT).unwrap();
    let control = owner.control.clone();
    assert_eq!(worker_tail.load(Ordering::SeqCst), 0);
    release.send(()).unwrap();
    drop(owner);
    assert!(control.is_cancelled());
    assert!(control.checkpoint().is_err());
    assert_eq!(worker_tail.load(Ordering::SeqCst), 1);
}
