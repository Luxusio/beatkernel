//! Public immutable PCM ownership, accounting and actual software-mixer evidence.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, Timestamp},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
    static FAIL_BYTES: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
fn observe(kind: usize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[kind] += 1;
                counts.set(value);
            });
        }
    });
}
fn refuse(size: usize, alignment: usize) -> bool {
    FAIL_BYTES
        .try_with(|bytes| {
            if bytes.get() == size && size != 0 && alignment == std::mem::align_of::<f32>() {
                bytes.set(0);
                true
            } else {
                false
            }
        })
        .unwrap_or(false)
}
// SAFETY: All actual allocation operations delegate unchanged arguments to System.
// Optional failure injection returns null only from allocation/reallocation, as
// GlobalAlloc permits. Bookkeeping uses thread-local scalar cells without allocation.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        observe(0);
        if refuse(layout.size(), layout.align()) {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        observe(0);
        if refuse(layout.size(), layout.align()) {
            return std::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        observe(1);
        if refuse(size, layout.align()) {
            return std::ptr::null_mut();
        }
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        observe(2);
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn track<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|enabled| enabled.set(true));
    let result = operation();
    TRACK.with(|enabled| enabled.set(false));
    (result, COUNTS.with(Cell::get))
}
const BITS: [u32; 8] = [
    0,
    0x8000_0000,
    1,
    0x8000_0001,
    0x7f7f_ffff,
    0xff7f_ffff,
    0x3f80_0001,
    0xbf00_0001,
];
fn limits() -> PcmLimits {
    PcmLimits::new(32, 128, 8).unwrap()
}
fn bits(sample: &PcmSample) -> Vec<u32> {
    sample
        .samples()
        .iter()
        .map(|value| value.to_bits())
        .collect()
}

#[test]
fn allocator_measurement_observes_all_three_memory_operations() {
    let ((), counts) = track(|| {
        let mut payload = Vec::with_capacity(1);
        payload.push(1_u8);
        payload.reserve_exact(1024);
        std::hint::black_box(&payload);
        drop(payload);
    });
    assert!(counts.iter().all(|count| *count > 0));
}

#[test]
fn sharing_preserves_constructor_pointer_bits_formats_and_consumer_after_owner_drop() {
    for (rate, channels) in [(44_100, 1), (96_000, 2), (u32::MAX, 4)] {
        let format = AudioFormat::new(rate, channels).unwrap();
        let payload = BITS.map(f32::from_bits).to_vec();
        let original_pointer = payload.as_ptr();
        let owner = PcmSample::new(format, payload, limits()).unwrap();
        assert_eq!(owner.samples().as_ptr(), original_pointer);
        let (shared, counts) = track(|| owner.try_share(limits()));
        let shared = shared.unwrap();
        assert_eq!(
            counts,
            [0, 0, 0],
            "sharing immutable PCM must not copy or allocate payload"
        );
        assert_eq!(shared.samples().as_ptr(), original_pointer);
        assert_eq!(shared.format(), format);
        assert_eq!(shared.frames(), 8 / usize::from(channels));
        drop(owner);
        assert_eq!(bits(&shared), BITS);
    }
}

#[test]
fn shallow_sharing_revalidates_limits_and_empty_assets_keep_logical_membership() {
    let format = AudioFormat::new(22_050, 2).unwrap();
    let owner = PcmSample::new(format, BITS.map(f32::from_bits).to_vec(), limits()).unwrap();
    let pointer = owner.samples().as_ptr();
    for bytes in [1, 4, 31] {
        assert!(matches!(
            owner.try_share(PcmLimits::new(bytes, 128, 8).unwrap()),
            Err(AudioError::PcmCapacity)
        ));
        assert_eq!(owner.samples().as_ptr(), pointer);
        assert_eq!(bits(&owner), BITS);
    }
    let tiny = PcmLimits::new(1, 1, 1).unwrap();
    let empty = PcmSample::new(format, Vec::new(), tiny).unwrap();
    let shared = empty.try_share(tiny).unwrap();
    assert_eq!(shared.frames(), 0);
    assert!(shared.try_into_samples().unwrap().is_empty());
    let mut bank = SampleBank::new(format, tiny).unwrap();
    bank.insert(SampleId(1), empty).unwrap();
    assert_eq!(bank.total_bytes(), 0);
    assert_eq!(bank.len(), 1);
    assert!(matches!(
        bank.insert(
            SampleId(2),
            PcmSample::new(format, Vec::new(), tiny).unwrap()
        ),
        Err(AudioError::PcmCapacity)
    ));
    let copy = bank.try_share(tiny).unwrap();
    drop(bank);
    assert_eq!(copy.len(), 1);
    assert_eq!(copy.get(SampleId(1)).unwrap().frames(), 0);
}

#[test]
fn bank_aliases_share_storage_but_each_identity_counts_toward_admission() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let mut bank = SampleBank::new(format, limits()).unwrap();
    let original = PcmSample::new(format, BITS.map(f32::from_bits).to_vec(), limits()).unwrap();
    let pointer = original.samples().as_ptr();
    bank.insert(SampleId(7), original.try_share(limits()).unwrap())
        .unwrap();
    bank.insert(SampleId(3), original).unwrap();
    assert_eq!(bank.total_bytes(), 64);
    assert_eq!(bank.len(), 2);
    for tighter in [
        PcmLimits::new(31, 128, 8).unwrap(),
        PcmLimits::new(32, 63, 8).unwrap(),
        PcmLimits::new(32, 64, 1).unwrap(),
    ] {
        assert!(matches!(
            bank.try_share(tighter),
            Err(AudioError::PcmCapacity)
        ));
        assert_eq!(bank.total_bytes(), 64);
    }
    let mut shared = bank.try_share(PcmLimits::new(32, 64, 2).unwrap()).unwrap();
    for id in [SampleId(3), SampleId(7)] {
        assert_eq!(shared.get(id).unwrap().samples().as_ptr(), pointer);
    }
    assert!(matches!(
        shared.insert(
            SampleId(3),
            bank.get(SampleId(3)).unwrap().try_share(limits()).unwrap()
        ),
        Err(AudioError::DuplicateSample)
    ));
    assert!(matches!(
        shared.insert(
            SampleId(9),
            bank.get(SampleId(3)).unwrap().try_share(limits()).unwrap()
        ),
        Err(AudioError::PcmCapacity)
    ));
    drop(bank);
    assert_eq!(bits(shared.get(SampleId(7)).unwrap()), BITS);
    assert_eq!(
        shared
            .into_samples()
            .map(|(id, _)| id.0)
            .collect::<Vec<_>>(),
        [3, 7]
    );
}

#[test]
fn unique_extraction_moves_payload_while_shared_extraction_is_independent_mutable_storage() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let original = PcmSample::new(format, BITS.map(f32::from_bits).to_vec(), limits()).unwrap();
    let pointer = original.samples().as_ptr();
    let (owned, counts) = track(|| original.try_into_samples());
    let owned = owned.unwrap();
    assert_eq!(owned.as_ptr(), pointer);
    assert_eq!(counts[0], 0);
    assert_eq!(counts[1], 0);
    let original = PcmSample::new(format, owned, limits()).unwrap();
    for fallible in [true, false] {
        let shared = original.try_share(limits()).unwrap();
        let mut mutable = if fallible {
            shared.try_into_samples().unwrap()
        } else {
            shared.into_samples()
        };
        assert_ne!(mutable.as_ptr(), original.samples().as_ptr());
        mutable[0] = 7.0;
        mutable[1] = 0.0;
        assert_eq!(bits(&original), BITS);
    }
}

#[test]
fn explicit_shared_payload_reserve_failure_returns_error_without_corrupting_owner() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let original = PcmSample::new(format, BITS.map(f32::from_bits).to_vec(), limits()).unwrap();
    let shared = original.try_share(limits()).unwrap();
    FAIL_BYTES.with(|bytes| bytes.set(32));
    let result = shared.try_into_samples();
    let remaining = FAIL_BYTES.with(|bytes| bytes.replace(0));
    assert_eq!(
        remaining, 0,
        "fixture must intercept actual f32 payload reserve"
    );
    assert!(matches!(result, Err(AudioError::AllocationFailed)));
    assert_eq!(bits(&original), BITS);
    assert_eq!(
        original
            .try_share(limits())
            .unwrap()
            .try_into_samples()
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn constructor_pays_one_cold_metadata_allocation_including_empty_assets() {
    let format = AudioFormat::new(1000, 1).unwrap();
    for payload in [Vec::new(), vec![0.25; 8]] {
        let pointer = payload.as_ptr();
        let (sample, counts) = track(|| PcmSample::new(format, payload, limits()));
        let sample = sample.unwrap();
        assert_eq!(
            counts,
            [1, 0, 0],
            "cold shared owner metadata cost is explicit"
        );
        assert_eq!(sample.samples().as_ptr(), pointer);
    }
}

#[test]
fn shared_and_independent_banks_render_equal_pcm_without_render_memory_operations() {
    let format = AudioFormat::new(1000, 1).unwrap();
    let sample = PcmSample::new(format, vec![0.25, 0.5, -0.75, 1.0], limits()).unwrap();
    let mut original = SampleBank::new(format, limits()).unwrap();
    original.insert(SampleId(1), sample).unwrap();
    let shared = original.try_share(limits()).unwrap();
    let mut copied = SampleBank::new(format, limits()).unwrap();
    copied
        .insert(
            SampleId(1),
            original
                .get(SampleId(1))
                .unwrap()
                .try_clone(limits())
                .unwrap(),
        )
        .unwrap();
    drop(original);
    let make = |bank| {
        let (mut producer, consumer) = command_queue(4).unwrap();
        producer
            .try_push(AudioCommand::Play {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            })
            .unwrap();
        let mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(9),
                Timestamp::ZERO,
                AudioLimits::new(4, 1, 4, 8, 4).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        (producer, mixer)
    };
    let (_shared_producer, mut shared_mixer) = make(shared);
    let (_copied_producer, mut copied_mixer) = make(copied);
    for expected in [[0.25, 0.5, -0.75, 1.0], [0.0; 4]] {
        let mut left = [99.0; 4];
        let mut right = [99.0; 4];
        let (shared_report, shared_counts) = track(|| shared_mixer.render(&mut left));
        let (copied_report, copied_counts) = track(|| copied_mixer.render(&mut right));
        assert_eq!(shared_counts, [0, 0, 0]);
        assert_eq!(copied_counts, [0, 0, 0]);
        shared_report.unwrap();
        copied_report.unwrap();
        assert_eq!(left, expected);
        assert_eq!(right, expected);
    }
}
