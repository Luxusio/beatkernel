//! Deferred filesystem fixtures. No sound asset or native playback is opened.
use crate::player_chart::{scan_library, scan_library_with, ChartLibrary, ScanStage};
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
            "beatkernel-cancellable-scan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn chart(&self, path: &str, text: &str) {
        fs::write(self.0.join(path), text).unwrap();
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn rows(library: &ChartLibrary) -> Vec<(PathBuf, String, String)> {
    library
        .entries
        .iter()
        .map(|entry| {
            (
                entry.path.clone(),
                entry.title.clone(),
                entry.artist.clone(),
            )
        })
        .collect()
}

#[test]
fn checkpoint_cancellation_never_returns_a_partial_catalog_and_preserves_sync_order_and_symlink_policy()
 {
    let root = Tree::new();
    fs::create_dir(root.0.join("nested")).unwrap();
    root.chart(
        "z.bms",
        "#TITLE ÉTOILE\n#ARTIST 作曲家\n#BPM 60\n#WAV01 missing.wav\n#00011:01\n",
    );
    root.chart("nested/c.BME", "#TITLE Alpha\n#ARTIST Zeta\n#BPM 60\n");
    root.chart("a.bml", "#TITLE Alpha\n#ARTIST Beta\n#BPM 60\n");
    root.chart("fallback.bms", "#BPM 60\n");
    fs::write(root.0.join("broken.bms"), [0xff, 0xfe]).unwrap();
    root.chart("not-a-chart.wav", "not an audio decoder input");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(root.0.join("z.bms"), root.0.join("linked.bms")).unwrap();
        symlink(root.0.join("nested"), root.0.join("linked-directory")).unwrap();
        assert!(scan_library_with(&root.0.join("linked-directory"), |_| true).is_err());
    }
    let mut observations = Vec::new();
    let complete = scan_library_with(&root.0, |progress| {
        observations.push(progress);
        true
    })
    .unwrap();
    assert_eq!(
        rows(&complete),
        vec![
            (root.0.join("a.bml"), "Alpha".into(), "Beta".into()),
            (root.0.join("nested/c.BME"), "Alpha".into(), "Zeta".into()),
            (
                root.0.join("fallback.bms"),
                "fallback".into(),
                String::new()
            ),
            (root.0.join("z.bms"), "ÉTOILE".into(), "作曲家".into()),
        ]
    );
    assert_eq!(complete.diagnostics.len(), 1);
    assert!(complete.diagnostics[0].contains("broken.bms"));
    let legacy = scan_library(&root.0).unwrap();
    assert_eq!(rows(&legacy), rows(&complete));
    assert_eq!(legacy.diagnostics, complete.diagnostics);
    assert_eq!(observations.last().unwrap().stage, ScanStage::Complete);
    assert_eq!(observations.last().unwrap().charts, 5);
    for pair in observations.windows(2) {
        assert!(pair[0].directories <= pair[1].directories);
        assert!(pair[0].entries <= pair[1].entries);
        assert!(pair[0].charts <= pair[1].charts);
        assert!(pair[0].bytes <= pair[1].bytes);
    }
    for stage in [
        ScanStage::Traversal,
        ScanStage::Reading,
        ScanStage::Parsing,
        ScanStage::Complete,
    ] {
        let mut reached = false;
        let result = scan_library_with(&root.0, |progress| {
            if progress.stage == stage {
                reached = true;
                false
            } else {
                true
            }
        });
        assert!(reached);
        assert!(
            result.is_err(),
            "even cancellation at Complete cannot expose a catalog"
        );
        assert_eq!(rows(&scan_library(&root.0).unwrap()), rows(&complete));
    }
    assert!(scan_library_with(&root.0.join("a.bml"), |_| true).is_err());
}

#[test]
fn cancellable_scan_keeps_existing_chart_byte_and_depth_bounds_with_visible_diagnostics() {
    let files = Tree::new();
    for index in 0..1025 {
        files.chart(&format!("{index:04}.bms"), "#TITLE Same\n#BPM 60\n");
    }
    let bounded = scan_library_with(&files.0, |_| true).unwrap();
    assert_eq!(bounded.entries.len(), 1024);
    assert_eq!(
        bounded.entries.first().unwrap().path,
        files.0.join("0000.bms")
    );
    assert_eq!(
        bounded.entries.last().unwrap().path,
        files.0.join("1023.bms")
    );
    assert!(
        bounded
            .diagnostics
            .iter()
            .any(|message| message.contains("chart-file limit"))
    );
    assert_eq!(rows(&scan_library(&files.0).unwrap()), rows(&bounded));
    let mut saw_admitted_prefix = false;
    assert!(
        scan_library_with(&files.0, |progress| {
            if progress.charts >= 2 {
                saw_admitted_prefix = true;
                false
            } else {
                true
            }
        })
        .is_err()
    );
    assert!(saw_admitted_prefix);

    let limits = Tree::new();
    let oversized = fs::File::create(limits.0.join("oversized.bms")).unwrap();
    oversized.set_len(8 * 1024 * 1024 + 1).unwrap();
    let mut deepest = limits.0.clone();
    for _ in 0..9 {
        deepest.push("nested");
        fs::create_dir(&deepest).unwrap();
    }
    fs::write(deepest.join("unvisited.bms"), "#BPM 60\n").unwrap();
    limits.chart(
        "valid.bms",
        "#TITLE Visible\n#BPM 60\n#WAV01 nonexistent.wav\n#00011:01\n",
    );
    let complete = scan_library_with(&limits.0, |_| true).unwrap();
    assert_eq!(complete.entries.len(), 1);
    assert_eq!(complete.entries[0].title, "Visible");
    assert!(
        complete
            .diagnostics
            .iter()
            .any(|message| message.contains("parser byte cap"))
    );
    assert!(
        complete
            .diagnostics
            .iter()
            .any(|message| message.contains("directory/depth limit"))
    );
    let legacy = scan_library(&limits.0).unwrap();
    assert_eq!(rows(&legacy), rows(&complete));
    assert_eq!(legacy.diagnostics, complete.diagnostics);
}
