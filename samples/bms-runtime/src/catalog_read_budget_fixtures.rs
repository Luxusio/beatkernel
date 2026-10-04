//! Deferred real filesystem mutation at scanner checkpoints; no fixture is run here.
use crate::player_chart::{scan_library, scan_library_with, ScanStage};
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
            "beatkernel-read-budget-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn scan_charges_actual_grown_shrunken_and_failed_prefixes_and_cancellation_returns_no_catalog() {
    let root = Tree::new();
    let grown = b"#TITLE Zulu\n#ARTIST Original\n#BPM 60\n#WAV01 not-opened.wav\n#00011:01\n";
    let shrunk = b"#TITLE Alpha\n#BPM 60\n";
    let malformed_text = [0xff, 0xfe, b'A', 0];
    let malformed_chart = b"#BPM 0\n";
    let tail = b"#TITLE Middle\n#BPM 60\n";
    root.write("a-grow.bms", b"#BPM 60\n");
    root.write("b-shrink.bms", vec![b' '; 4096]);
    root.write("c-decode.bms", malformed_text);
    root.write("d-parse.bms", malformed_chart);
    root.write("e-disappear.bms", b"#BPM 60\n");
    root.write("f-tail.bms", tail);
    let oversized = fs::File::create(root.0.join("g-too-large.bms")).unwrap();
    oversized.set_len(8 * 1024 * 1024 + 1).unwrap();
    let mut readings = Vec::new();
    let mut parsed_bytes = Vec::new();
    let mut observations = Vec::new();
    let library = scan_library_with(&root.0, |progress| {
        observations.push(progress);
        if progress.stage == ScanStage::Reading {
            readings.push((progress.charts, progress.bytes));
            match progress.charts {
                1 => root.write("a-grow.bms", grown),
                2 => root.write("b-shrink.bms", shrunk),
                5 => fs::remove_file(root.0.join("e-disappear.bms")).unwrap(),
                _ => {}
            }
        }
        if progress.stage == ScanStage::Parsing {
            parsed_bytes.push(progress.bytes);
        }
        true
    })
    .unwrap();
    let a = grown.len() as u64;
    let b = a + shrunk.len() as u64;
    let c = b + malformed_text.len() as u64;
    let d = c + malformed_chart.len() as u64;
    let total = d + tail.len() as u64;
    assert_eq!(readings, [(1, 0), (2, a), (3, b), (4, c), (5, d), (6, d)]);
    assert_eq!(parsed_bytes, [a, b, c, d, d, total]);
    assert_eq!(observations.last().unwrap().bytes, total);
    assert_eq!(observations.last().unwrap().stage, ScanStage::Complete);
    assert_eq!(observations.last().unwrap().charts, 7);
    assert_eq!(
        library
            .entries
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "Middle", "Zulu"]
    );
    assert_eq!(
        library
            .entries
            .iter()
            .map(|entry| entry.path.file_name().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        ["b-shrink.bms", "f-tail.bms", "a-grow.bms"]
    );
    assert_eq!(library.diagnostics.len(), 4);
    for name in [
        "c-decode.bms",
        "d-parse.bms",
        "e-disappear.bms",
        "g-too-large.bms",
    ] {
        assert!(
            library
                .diagnostics
                .iter()
                .any(|message| message.contains(name))
        );
    }
    assert!(
        observations
            .windows(2)
            .all(|pair| pair[0].bytes <= pair[1].bytes)
    );
    let synchronous = scan_library(&root.0).unwrap();
    assert_eq!(
        synchronous
            .entries
            .iter()
            .map(|entry| (&entry.path, &entry.title))
            .collect::<Vec<_>>(),
        library
            .entries
            .iter()
            .map(|entry| (&entry.path, &entry.title))
            .collect::<Vec<_>>()
    );
    let mut cancelled = None;
    let result = scan_library_with(&root.0, |progress| {
        if progress.stage == ScanStage::Parsing && progress.charts == 2 {
            cancelled = Some(progress);
            return false;
        }
        true
    });
    assert!(
        result.is_err(),
        "a cancelled scan cannot expose its already read or parsed prefix"
    );
    assert_eq!(cancelled.unwrap().bytes, b);
}

#[test]
fn growing_files_obey_the_exact_aggregate_budget_and_one_detection_byte_stops_before_parse_or_later_read()
 {
    const PER_CHART: usize = 8 * 1024 * 1024;
    const TOTAL: u64 = 64 * 1024 * 1024;
    for overflow in [false, true] {
        let root = Tree::new();
        for index in 1..=8 {
            root.write(&format!("{index:02}.bms"), b"#BPM 60\n");
        }
        if overflow {
            root.write("09-never-read.bms", b"#TITLE Later\n#BPM 60\n");
        }
        let mut readings = Vec::new();
        let mut parsing = Vec::new();
        let mut completed = None;
        let library = scan_library_with(&root.0, |progress| {
            if progress.stage == ScanStage::Reading {
                readings.push((progress.charts, progress.bytes));
                assert!(
                    progress.charts <= 8,
                    "the aggregate detection byte must stop all later reads"
                );
                let header = format!("#TITLE Chart {:02}\n#BPM 60\n", progress.charts);
                let mut bytes = header.into_bytes();
                bytes.resize(
                    PER_CHART + usize::from(overflow && progress.charts == 8),
                    b' ',
                );
                // Keep padding within the real parser's physical line and line
                // count limits, independently of the aggregate read budget.
                for index in (4095..bytes.len()).step_by(4096) {
                    bytes[index] = b'\n';
                }
                root.write(&format!("{:02}.bms", progress.charts), bytes);
            }
            if progress.stage == ScanStage::Parsing {
                parsing.push((progress.charts, progress.bytes));
            }
            if progress.stage == ScanStage::Complete {
                completed = Some(progress);
            }
            true
        })
        .unwrap();
        assert_eq!(
            readings,
            (0..8)
                .map(|index| (index + 1, index as u64 * PER_CHART as u64))
                .collect::<Vec<_>>()
        );
        let accepted = if overflow { 7 } else { 8 };
        assert_eq!(library.entries.len(), accepted);
        assert_eq!(
            library
                .entries
                .iter()
                .map(|entry| entry.title.clone())
                .collect::<Vec<_>>(),
            (1..=accepted)
                .map(|index| format!("Chart {index:02}"))
                .collect::<Vec<_>>()
        );
        let mut after_read = (1..=8)
            .map(|index| (index, index as u64 * PER_CHART as u64))
            .collect::<Vec<_>>();
        after_read.last_mut().unwrap().1 += u64::from(overflow);
        // Parsing is the cancellable boundary after reading, not evidence of
        // parser invocation. The overflowing candidate produces no catalog row.
        assert_eq!(parsing, after_read);
        assert_eq!(completed.unwrap().bytes, TOTAL + u64::from(overflow));
        if overflow {
            assert_eq!(library.diagnostics.len(), 1);
            assert!(library.diagnostics[0].contains("aggregate chart-byte limit"));
            assert_eq!(
                fs::read(root.0.join("09-never-read.bms")).unwrap(),
                b"#TITLE Later\n#BPM 60\n"
            );
        } else {
            assert!(library.diagnostics.is_empty());
        }
    }
}
