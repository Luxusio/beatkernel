//! Heap requests for actual authority pin reads; not CPU, native latency, or callback proof.
use beatkernel::{
    audio::{InterpolatedTargetTime, OutputFrameBasis, TargetFrameBasis, TargetTime},
    input::DeviceId,
    time::{ClockDomainId, ClockPair, ClockPoint, Duration, ExtrapolationPolicy, Timestamp},
};
use beatkernel_bms_runtime::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
        AudioAuthorityError,
    },
    local_input::InputMerger,
    native_output_coordinate::NativeOutputCoordinate,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    cmp::Ordering,
};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static REQUESTS: Cell<usize> = const { Cell::new(0) };
}
fn request() {
    let _ = TRACK.try_with(|active| {
        if active.get() {
            let _ = REQUESTS.try_with(|count| count.set(count.get() + 1));
        }
    });
}
struct Allocator;
// SAFETY: Every pointer, size, and layout is passed unchanged to System. TLS holds scalars.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        request();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        request();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        request();
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        request();
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        TRACK.with(|active| active.set(false));
    }
}
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
    TRACK.with(|active| assert!(!active.get()));
    REQUESTS.with(|count| count.set(0));
    TRACK.with(|active| active.set(true));
    let guard = Guard;
    let result = f();
    drop(guard);
    (result, REQUESTS.with(Cell::get))
}
fn calibrate() {
    let (_, requests) = measure(|| {
        let mut allocation = Vec::<u64>::with_capacity(std::hint::black_box(4));
        allocation.push(std::hint::black_box(1));
        allocation.reserve(std::hint::black_box(128));
        drop(allocation);
    });
    assert!(
        requests >= 3,
        "allocator must observe allocation, reallocation, and deallocation"
    );
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn bases() -> [AudioAuthorityCoordinateBasis; 3] {
    let source = OutputFrameBasis::new(point(2, 100), 3, 2).unwrap();
    [
        AudioAuthorityCoordinateBasis::SourceFrame(source),
        AudioAuthorityCoordinateBasis::TargetFrame(
            TargetFrameBasis::new(point(2, 100), TargetTime::new(0, 1, 3).unwrap(), 7).unwrap(),
        ),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: source,
            frequency: u64::MAX,
        },
    ]
}
fn setup(basis: AudioAuthorityCoordinateBasis, policy: ExtrapolationPolicy) -> AudioAuthority {
    let origin = match basis {
        AudioAuthorityCoordinateBasis::SourceFrame(b)
        | AudioAuthorityCoordinateBasis::SourceCounter { basis: b, .. } => {
            b.point_at_stream_frame(0).unwrap()
        }
        AudioAuthorityCoordinateBasis::TargetFrame(b) => b.point_at_stream_frame(0).unwrap(),
    };
    AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: 8,
            max_observation_age: Duration::from_nanos(1_000),
            input_extrapolation: policy,
            ..AudioAuthorityConfig::default()
        },
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: origin,
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap()
}
fn observe(owner: &mut AudioAuthority, units: u64, at: i64) {
    let basis = owner.coordinate_basis().unwrap();
    let (coordinate, source) = match basis {
        AudioAuthorityCoordinateBasis::SourceFrame(b) => (
            NativeOutputCoordinate::StreamFrame { frame: units },
            b.point_at_stream_frame(units).unwrap(),
        ),
        AudioAuthorityCoordinateBasis::TargetFrame(b) => (
            NativeOutputCoordinate::StreamFrame { frame: units },
            b.point_at_stream_frame(units).unwrap(),
        ),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: b,
            frequency,
        } => (
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            },
            b.point_at_native_counter(units, frequency).unwrap(),
        ),
    };
    owner
        .observe_coordinate(
            7,
            ClockPair {
                source,
                target: point(1, at),
            },
            coordinate,
        )
        .unwrap();
}
fn measure_pin(owner: &AudioAuthority, expected: InterpolatedTargetTime) {
    let before = (
        owner.history_len(),
        owner.latest_coordinate_observation(),
        owner.acquired_prefix(),
        owner.closed_host_prefix(),
        owner.committed_input_host(),
        owner.committed_operation(),
        owner.committed_presentation(),
    );
    let (last, requests) = measure(|| {
        let mut last = expected;
        for _ in 0..128 {
            last = std::hint::black_box(owner)
                .future_input_retention_query()
                .unwrap();
            assert_eq!(last.compare_interpolated_time(expected), Ordering::Equal);
        }
        last
    });
    assert_eq!(requests, 0);
    assert_eq!(last.compare_interpolated_time(expected), Ordering::Equal);
    assert_eq!(
        (
            owner.history_len(),
            owner.latest_coordinate_observation(),
            owner.acquired_prefix(),
            owner.closed_host_prefix(),
            owner.committed_input_host(),
            owner.committed_operation(),
            owner.committed_presentation()
        ),
        before
    );
}

#[test]
fn cold_first_and_real_closed_anchor_pin_reads_have_zero_heap_requests() {
    calibrate();
    for basis in bases() {
        for policy in [
            ExtrapolationPolicy::Forbid,
            ExtrapolationPolicy::Bounded {
                before: Duration::from_nanos(5),
                after: Duration::ZERO,
            },
        ] {
            let mut owner = setup(basis, policy);
            let cold = owner.future_input_retention_query().unwrap();
            measure_pin(&owner, cold);
            observe(&mut owner, 2, 100);
            observe(&mut owner, 5, 110);
            let first = owner.future_input_retention_query().unwrap();
            measure_pin(&owner, first);
            owner.record_acquired_prefix(point(1, 110)).unwrap();
            let mut merger =
                InputMerger::new(ClockDomainId(1), point(1, 0), vec![DeviceId(9)], 8).unwrap();
            let prepared = owner
                .prepare_coordinate_frontier(point(1, 110), &merger)
                .unwrap()
                .unwrap();
            measure_pin(&owner, first);
            owner
                .commit_coordinate_frontier(prepared, point(1, 110), &mut merger)
                .unwrap();
            measure_pin(&owner, prepared.query());
        }
    }
}

#[test]
fn scalar_mode_refusal_requests_no_heap_and_keeps_original_empty_state() {
    calibrate();
    let coordinate = setup(bases()[0], ExtrapolationPolicy::Forbid);
    let scalar = AudioAuthority::new(coordinate.config(), coordinate.epoch()).unwrap();
    let (result, requests) = measure(|| {
        for _ in 0..128 {
            assert_eq!(
                std::hint::black_box(&scalar)
                    .future_input_retention_query()
                    .unwrap_err(),
                AudioAuthorityError::CoordinateModeMismatch
            );
        }
        scalar.history_len()
    });
    assert_eq!(requests, 0);
    assert_eq!(result, 0);
    assert_eq!(scalar.latest_observation(), None);
    assert_eq!(scalar.acquired_prefix(), None);
    assert_eq!(scalar.closed_host_prefix(), None);
}
