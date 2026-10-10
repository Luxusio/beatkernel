//! Public real-filesystem catalog admission remains independent of media readiness.
use beatkernel_bms_runtime::player_chart::{
    scan_library, scan_library_streamed_with, scan_library_with, ScanStage,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "beatkernel-catalog-loading-public-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn chart(&self, name: &str, title: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(
            &path,
            format!("#TITLE {title}\n#BPM 120\n#WAV01 absent.wav\n#00011:01\n"),
        )
        .unwrap();
        path
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn actual_parsed_catalog_is_sorted_without_acquiring_missing_media() {
    let tree = Tree::new();
    let last = tree.chart("first-path.bms", "Zulu 音");
    let first = tree.chart("last-path.bms", "Alpha 별");
    let library = scan_library(&tree.0).unwrap();
    assert_eq!(
        library
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        [first, last]
    );
    assert!(library.diagnostics.is_empty());
    assert!(!tree.0.join("absent.wav").exists());
}

#[test]
fn empty_and_invalid_metadata_have_explicit_distinct_results_and_recovery() {
    let tree = Tree::new();
    let empty = scan_library(&tree.0).unwrap();
    assert!(empty.entries.is_empty() && empty.diagnostics.is_empty());
    fs::write(tree.0.join("broken.bms"), "#BPM 0\n#00011:01\n").unwrap();
    let invalid = scan_library(&tree.0).unwrap();
    assert!(invalid.entries.is_empty());
    assert_eq!(invalid.diagnostics.len(), 1);
    fs::remove_file(tree.0.join("broken.bms")).unwrap();
    let path = tree.chart("recovered.bms", "Recovered");
    let recovered = scan_library(&tree.0).unwrap();
    assert_eq!(recovered.entries.len(), 1);
    assert_eq!(recovered.entries[0].path, path);
    assert!(recovered.diagnostics.is_empty());
}

#[test]
fn cancelled_real_scan_refuses_final_catalog_and_fresh_retry_still_succeeds() {
    let tree = Tree::new();
    for index in 0..130 {
        tree.chart(&format!("{index:03}.bms"), &format!("Chart {index:03}"));
    }
    let mut boundaries = Vec::new();
    let refused = scan_library_with(&tree.0, |progress| {
        boundaries.push(progress.stage);
        progress.stage != ScanStage::Parsing
    });
    assert!(refused.err().unwrap().to_string().contains("cancel"));
    assert!(boundaries.contains(&ScanStage::Parsing));
    assert!(!boundaries.contains(&ScanStage::Complete));
    assert_eq!(scan_library(&tree.0).unwrap().entries.len(), 130);
}

#[test]
fn streamed_real_parse_exposes_first_canonical_path_before_remaining_scan_finishes() {
    use std::{sync::mpsc, thread, time::Duration};
    let tree = Tree::new();
    for index in 0..130 {
        tree.chart(&format!("{index:03}.bms"), &format!("Chart {index:03}"));
    }
    let root = tree.0.clone();
    let (entered, admitted) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let mut count = 0;
        scan_library_streamed_with(
            &root,
            |_| true,
            |entry| {
                count += 1;
                if count == 1 {
                    entered.send(entry.clone()).unwrap();
                    gate.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                true
            },
        )
        .unwrap()
    });
    let first = admitted.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!worker.is_finished());
    assert!(first.path.is_absolute() && first.path.exists());
    assert!(first.title.starts_with("Chart "));
    assert!(!tree.0.join("absent.wav").exists());
    release.send(()).unwrap();
    let final_library = worker.join().unwrap();
    assert_eq!(final_library.entries.len(), 130);
    assert!(final_library
        .entries
        .iter()
        .any(|entry| entry.path == first.path));
}

#[test]
fn refused_stream_publication_cancels_scan_without_fabricating_final_success() {
    let tree = Tree::new();
    tree.chart("first.bms", "First");
    tree.chart("second.bms", "Second");
    let mut calls = 0;
    let result = scan_library_streamed_with(
        &tree.0,
        |_| true,
        |_| {
            calls += 1;
            false
        },
    );
    assert!(result.err().unwrap().to_string().contains("cancel"));
    assert_eq!(calls, 1);
    assert_eq!(scan_library(&tree.0).unwrap().entries.len(), 2);
}
