//! Allocation and identity regressions for the production note serializer.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

use beatkernel::{
    audio::SampleId,
    chart::ObjectMetadata,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeWindow},
    time::Duration,
};
use beatkernel_bms::{BmsChart, ParseOptions, parse};

#[path = "../src/parser_metadata.rs"]
mod metadata;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 2]> = const { Cell::new([0; 2]) };
}

struct Allocator;

fn count(kind: usize) {
    let _ = ENABLED.try_with(|enabled| {
        if enabled.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}

// SAFETY: System receives unchanged caller arguments. Instrumentation only uses
// allocation-free thread-local scalar cells in this integration test binary.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        // SAFETY: Caller supplies a live System allocation, matching layout,
        // and valid nonzero replacement size.
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Caller supplies the live allocation and its original layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

struct TrackingGuard(bool);

impl Drop for TrackingGuard {
    fn drop(&mut self) {
        ENABLED.with(|enabled| enabled.set(self.0));
    }
}

fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 2]) {
    // Warm both TLS cells before starting the allocation window.
    let previous = ENABLED.with(Cell::get);
    COUNTS.with(|counts| counts.set([0; 2]));
    assert!(!previous, "allocation windows must not nest");
    let guard = TrackingGuard(previous);
    ENABLED.with(|enabled| enabled.set(true));
    let result = operation();
    drop(guard); // Disable before assertions or the returned vector's drop.
    (result, COUNTS.with(Cell::get))
}

// Exact prior inline builder, retained only as a growth-positive control.
fn previous_metadata(lane: u8, head: u16, line: usize, tail: Option<SampleId>) -> Vec<u8> {
    let mut bytes = vec![1, lane];
    bytes.extend(head.to_le_bytes());
    bytes.extend((line as u64).to_le_bytes());
    if let Some(tail) = tail {
        bytes.push(1);
        bytes.extend((tail.0 as u16).to_le_bytes());
    } else {
        bytes.push(0);
    }
    bytes
}

#[test]
fn allocator_probe_observes_retained_allocation_and_growth() {
    let (bytes, counts) = tracked(|| {
        let mut bytes = Vec::with_capacity(black_box(1));
        bytes.push(black_box(7_u8));
        bytes.reserve_exact(black_box(1024));
        black_box(bytes)
    });
    assert!(counts[0] > 0);
    assert!(counts[1] > 0);
    assert_eq!(bytes, [7]);
}

#[test]
fn tracking_guard_restores_disabled_state_on_unwind() {
    let result = std::panic::catch_unwind(|| {
        tracked(|| panic!("intentional allocation guard unwind"));
    });
    assert!(result.is_err());
    assert!(!ENABLED.with(Cell::get));
    let (bytes, counts) = tracked(|| black_box(metadata::note_metadata(0x11, 1, 1, None)));
    assert_eq!(counts, [1, 0]);
    assert_eq!(bytes.len(), 13);
}

fn assert_serialization(lane: u8, head: u16, line: usize, tail: Option<SampleId>, golden: &[u8]) {
    let inputs = black_box((lane, head, line, tail));
    let (bytes, counts) = tracked(|| {
        black_box(metadata::note_metadata(
            inputs.0, inputs.1, inputs.2, inputs.3,
        ))
    });
    assert_eq!(counts, [1, 0], "initial allocation only, without growth");
    assert_eq!(bytes, golden);
    let (previous, old_counts) =
        tracked(|| black_box(previous_metadata(inputs.0, inputs.1, inputs.2, inputs.3)));
    assert!(old_counts[0] > 0);
    assert!(old_counts[1] > 0, "prior two-byte vector grows");
    assert_eq!(previous, golden);
    assert_eq!(bytes, previous);
}

#[test]
fn exact_literal_bytes_and_one_allocation_cover_tail_presence_and_casts() {
    assert_serialization(0, 0, 0, None, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_serialization(
        0x26,
        0x1234,
        0x0102,
        Some(SampleId(0)),
        &[1, 0x26, 0x34, 0x12, 2, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0],
    );
    assert_serialization(
        0xff,
        u16::MAX,
        0,
        Some(SampleId(u64::MAX)),
        &[1, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xff, 0xff],
    );
    assert_serialization(
        0x11,
        1,
        1,
        Some(SampleId(0x1234_5678)),
        &[1, 0x11, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0x78, 0x56],
    );
}

#[test]
fn platform_line_extrema_keep_unsigned_64_bit_encoding() {
    #[cfg(target_pointer_width = "64")]
    {
        assert_serialization(
            0x19,
            0x1234,
            0x0102_0304_0506_0708,
            None,
            &[1, 0x19, 0x34, 0x12, 8, 7, 6, 5, 4, 3, 2, 1, 0],
        );
        assert_serialization(
            0xff,
            u16::MAX,
            usize::MAX,
            Some(SampleId(u64::MAX)),
            &[
                1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1, 0xff, 0xff,
            ],
        );
    }
    #[cfg(target_pointer_width = "32")]
    assert_serialization(
        0xff,
        u16::MAX,
        usize::MAX,
        Some(SampleId(u64::MAX)),
        &[
            1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 1, 0xff, 0xff,
        ],
    );
}

fn engine_hash(chart: &BmsChart) -> u64 {
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(1),
            late: Duration::from_nanos(1),
        }],
        Duration::ZERO,
    )
    .unwrap();
    JudgeEngine::new(chart.compile().unwrap().chart, chart.rules(), profile)
        .unwrap()
        .stable_hash()
        .unwrap()
}

#[test]
fn public_parse_compile_and_judge_identity_preserve_literal_note_metadata() {
    // Each golden encodes the physical head line, not a generated note ordinal.
    let fixtures: [(&str, &[u8], bool, Option<u64>, usize); 5] = [
        (
            "; physical line\n#BPM 60\n#WAV01 head.wav\n#00011:01",
            &[1, 0x11, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0],
            false,
            None,
            4,
        ),
        (
            "#BPM 60\n#LNTYPE 1\n#WAV01 head.wav\n; physical line\n#00051:0102",
            &[1, 0x11, 1, 0, 5, 0, 0, 0, 0, 0, 0, 0, 1, 2, 0],
            true,
            Some(2),
            5,
        ),
        (
            "#BPM 60\n#LNTYPE 1\n#WAV01 head.wav\n#WAV02 tail.wav\n#00051:0102",
            &[1, 0x11, 1, 0, 5, 0, 0, 0, 0, 0, 0, 0, 1, 2, 0],
            true,
            Some(2),
            5,
        ),
        (
            "#BPM 60\n#LNOBJ ZZ\n#WAV01 head.wav\n; physical line\n#00011:01ZZ",
            &[1, 0x11, 1, 0, 5, 0, 0, 0, 0, 0, 0, 0, 1, 0x0f, 5],
            true,
            Some(1295),
            5,
        ),
        (
            "#BPM 60\n#LNTYPE 2\n#WAV01 head.wav\n; physical line\n#00051:0100",
            &[1, 0x11, 1, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0],
            true,
            None,
            5,
        ),
    ];
    for (text, golden, hold, tail, line) in fixtures {
        let parsed = parse(text, ParseOptions::default()).unwrap();
        assert_eq!(parsed.notes.len(), 1);
        assert_eq!(parsed.source.objects.len(), 1);
        assert_eq!(parsed.notes[0].line, line);
        assert_eq!(parsed.notes[0].tail_sample.map(|sample| sample.0), tail);
        assert_eq!(parsed.source.objects[0].end.is_some(), hold);
        assert_eq!(parsed.source.objects[0].metadata.0, golden);
        let compiled = parsed.compile().unwrap();
        assert_eq!(compiled.chart.objects()[0].metadata.0, golden);
        let mut reference = parsed.clone();
        reference.source.objects[0].metadata = ObjectMetadata(golden.to_vec());
        assert_eq!(compiled, reference.compile().unwrap());
        assert_eq!(engine_hash(&parsed), engine_hash(&reference));
        reference.source.objects[0].metadata.0[0] ^= 1;
        assert_ne!(engine_hash(&parsed), engine_hash(&reference));
    }
}
