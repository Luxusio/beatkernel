//! Finite preparation allocation-request measurements, not process-memory peaks.
use beatkernel::audio::{AudioFormat, PcmLimits, PcmSample};
use beatkernel_bms_runtime::{
    asset_paths::AssetPathPolicy, asset_source::MemoryFiles, prepare_from_source, AssetDecoder,
    ChannelPolicy, DefaultAssetDecoder,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    error::Error,
    path::Path,
};

#[path = "../src/flac_fixture.rs"]
#[allow(dead_code)]
mod flac_fixture;
#[path = "../src/mp3_fixture.rs"]
#[allow(dead_code)]
mod mp3_fixture;
#[path = "../src/vorbis_fixture.rs"]
#[allow(dead_code)]
mod vorbis_fixture;

#[derive(Clone, Copy, Debug, Default)]
struct Requests {
    count: usize,
    total: usize,
    largest: usize,
}
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static REQUESTS: Cell<Requests> = const { Cell::new(Requests { count: 0, total: 0, largest: 0 }) };
}
fn record(bytes: usize) {
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            let _ = REQUESTS.try_with(|requests| {
                let old = requests.get();
                requests.set(Requests {
                    count: old.count.saturating_add(1),
                    total: old.total.saturating_add(bytes),
                    largest: old.largest.max(bytes),
                });
            });
        }
    });
}
struct Allocator;
// SAFETY: Pointers/layouts forward unchanged to System. Hooks use only scalar
// TLS operations and never allocate, unwind, inspect or retain allocation data.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc provides a valid allocation layout.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc provides a valid allocation layout.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record(layout.size());
        }
        pointer
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: The caller supplies a live System allocation and valid size.
        let result = unsafe { System.realloc(pointer, layout, size) };
        // Count the complete successful new request, not live-byte growth.
        if !result.is_null() {
            record(size);
        }
        result
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The caller supplies the allocation's original pointer/layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
struct Measurement;
impl Drop for Measurement {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
    }
}
fn measure<T>(operation: impl FnOnce() -> T) -> (T, Requests) {
    ACTIVE.with(|active| assert!(!active.get(), "nested measurement"));
    REQUESTS.with(|requests| requests.set(Requests::default()));
    ACTIVE.with(|active| active.set(true));
    let guard = Measurement;
    let result = operation();
    drop(guard);
    (result, REQUESTS.with(Cell::get))
}
fn limits(cap: usize) -> PcmLimits {
    PcmLimits::new(cap, cap, 1).unwrap()
}
fn decode(bytes: &[u8], cap: usize) -> Result<PcmSample, Box<dyn Error>> {
    DefaultAssetDecoder.decode(Path::new("content.bin"), bytes, limits(cap))
}
fn wav(samples: usize) -> Vec<u8> {
    let data = u32::try_from(samples * 2).unwrap();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24000u32.to_le_bytes());
    bytes.extend_from_slice(&48000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes.resize(bytes.len() + data as usize, 0);
    bytes
}
fn originals() -> Vec<(&'static str, Vec<u8>, usize)> {
    vec![
        ("wav", wav(1), 4),
        ("flac", flac_fixture::flac16(24000, 1, &[0], Some(1)), 4),
        ("vorbis", vorbis_fixture::silence(1, 48), 192),
        ("mp3", mp3_fixture::silence(1, 1), 2304),
    ]
}

#[test]
fn accepted_decode_and_whole_preparation_requests_have_named_corpus_bounds() {
    for (name, encoded, pcm_bytes) in originals() {
        let (decoded, stats) = measure(|| decode(&encoded, pcm_bytes));
        assert_eq!(decoded.unwrap().samples().len() * 4, pcm_bytes);
        assert!(stats.count > 0 && stats.count < 512, "{name}: {stats:?}");
        assert!(
            stats.total < 1024 * 1024 && stats.largest < 256 * 1024,
            "{name}: {stats:?}"
        );
        println!("accepted-decode {name}: {stats:?}");
        let chart = b"#BPM 120\n#WAV01 tone.bin\n#00011:01\n";
        let mut files = MemoryFiles::new(Default::default()).unwrap();
        files.insert("chart.bms", chart.to_vec()).unwrap();
        files.insert("tone.bin", encoded).unwrap();
        let source = files.scope("chart.bms").unwrap();
        let output = AudioFormat::new(48000, 1).unwrap();
        let cap = limits(pcm_bytes);
        let (prepared, stats) = measure(|| {
            prepare_from_source(
                chart,
                &source,
                output,
                cap,
                ChannelPolicy::Exact,
                &DefaultAssetDecoder,
                AssetPathPolicy::Exact,
                0,
                None,
            )
        });
        assert_eq!(prepared.unwrap().bank.total_bytes(), pcm_bytes);
        assert!(
            stats.count < 1024 && stats.total < 2 * 1024 * 1024 && stats.largest < 256 * 1024,
            "{name}: {stats:?}"
        );
        println!("accepted-prepare {name}: {stats:?}");
    }
}

#[test]
fn declared_pcm_refusals_do_not_request_the_rejected_full_output() {
    let mut ogg = vorbis_fixture::silence(1, 48);
    let last = vorbis_fixture::pages(&ogg).last().unwrap().0;
    ogg[last + 6..last + 14].copy_from_slice(&(1u64 << 20).to_le_bytes());
    vorbis_fixture::reseal_page(&mut ogg, last);
    let cases = vec![
        ("wav", wav(1024), 4096),
        (
            "flac",
            flac_fixture::flac16(24000, 1, &[0; 16], Some(1 << 20)),
            4 << 20,
        ),
        ("vorbis", ogg, 4 << 20),
        ("mp3", mp3_fixture::silence(1, 64), 64 * 576 * 4),
    ];
    for (name, encoded, rejected_pcm) in cases {
        let (result, stats) = measure(|| decode(&encoded, 4095));
        assert!(result.is_err(), "{name}");
        assert!(
            stats.largest < rejected_pcm && stats.largest < 4096 && stats.total < 16384,
            "{name}: {stats:?}"
        );
        println!("declared-refusal {name}: {stats:?}");
    }
}

fn ignored_metadata(name: &str, size: usize) -> Vec<u8> {
    match name {
        "wav" => {
            let original = wav(1);
            let mut bytes = original[..36].to_vec();
            bytes.extend_from_slice(b"JUNK");
            bytes.extend_from_slice(&(size as u32).to_le_bytes());
            bytes.resize(bytes.len() + size, 0);
            bytes.extend_from_slice(&original[36..]);
            let extent = (bytes.len() - 8) as u32;
            bytes[4..8].copy_from_slice(&extent.to_le_bytes());
            bytes
        }
        "flac" => {
            let original = flac_fixture::flac16(24000, 1, &[0], Some(1));
            let mut bytes = original[..42].to_vec();
            bytes[4] = 0;
            bytes.extend_from_slice(&[0x84, (size >> 16) as u8, (size >> 8) as u8, size as u8]);
            bytes.resize(bytes.len() + size, 0);
            bytes.extend_from_slice(&original[42..]);
            bytes
        }
        "mp3" => {
            let mut bytes = b"ID3\x04\0\0".to_vec();
            bytes.extend_from_slice(&[
                ((size >> 21) & 127) as u8,
                ((size >> 14) & 127) as u8,
                ((size >> 7) & 127) as u8,
                (size & 127) as u8,
            ]);
            bytes.resize(bytes.len() + size, 0);
            bytes.extend_from_slice(&mp3_fixture::silence(1, 1));
            bytes
        }
        _ => unreachable!(),
    }
}

#[test]
fn ignored_metadata_growth_does_not_copy_the_large_body_during_decode() {
    for (name, cap) in [("wav", 4), ("flac", 4), ("mp3", 2304)] {
        let small = ignored_metadata(name, 16);
        let large = ignored_metadata(name, 128 * 1024);
        let (small_result, small_stats) = measure(|| decode(&small, cap));
        let (large_result, large_stats) = measure(|| decode(&large, cap));
        assert_eq!(
            small_result.unwrap().samples(),
            large_result.unwrap().samples()
        );
        assert!(
            large_stats.total <= small_stats.total + 1024 && large_stats.largest < 128 * 1024,
            "{name}: {small_stats:?} => {large_stats:?}"
        );
        println!("ignored-metadata {name}: {small_stats:?} => {large_stats:?}");
    }
}

fn vendor(size: usize) -> Vec<u8> {
    let mut bytes = vorbis_fixture::silence(1, 48);
    let (offset, old_size) = vorbis_fixture::pages(&bytes)[1];
    let mut packet = b"\x03vorbis".to_vec();
    packet.extend_from_slice(&(size as u32).to_le_bytes());
    packet.resize(packet.len() + size, b'v');
    packet.extend_from_slice(&0u32.to_le_bytes());
    packet.push(1);
    let segments = packet.len() / 255 + 1;
    assert!(segments <= 255);
    let mut page = bytes[offset..offset + 27].to_vec();
    page[26] = segments as u8;
    page.extend(std::iter::repeat_n(255, segments - 1));
    page.push((packet.len() % 255) as u8);
    page.extend_from_slice(&packet);
    vorbis_fixture::reseal_page(&mut page, 0);
    bytes.splice(offset..offset + old_size, page);
    bytes
}

#[test]
fn parsed_vorbis_metadata_has_separate_measured_storage_cost() {
    let small = vendor(16);
    let large = vendor(8192);
    let (a, small_stats) = measure(|| decode(&small, 192));
    let (b, large_stats) = measure(|| decode(&large, 192));
    assert_eq!(a.unwrap().samples(), b.unwrap().samples());
    assert!(
        large_stats.largest >= 8192 && large_stats.total > small_stats.total,
        "{small_stats:?} => {large_stats:?}"
    );
    assert!(
        large_stats.count < 512
            && large_stats.total < 1024 * 1024
            && large_stats.largest < 256 * 1024,
        "{large_stats:?}"
    );
    println!("parsed-vorbis-metadata: {small_stats:?} => {large_stats:?}");
}

#[test]
fn unwind_disables_measurement_before_the_next_operation() {
    let failed = std::panic::catch_unwind(|| measure(|| panic!("original measurement failure")));
    assert!(failed.is_err());
    assert!(!ACTIVE.with(Cell::get));
    let (_, stats) = measure(|| ());
    assert_eq!((stats.count, stats.total, stats.largest), (0, 0, 0));
}
