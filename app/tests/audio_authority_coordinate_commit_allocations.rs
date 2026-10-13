//! Heap requests for actual coordinate commits; not a native latency benchmark.
use beatkernel::{
    audio::OutputFrameBasis,
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
};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static REQUESTS: Cell<usize> = const { Cell::new(0) };
}
fn request() {
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            let _ = REQUESTS.try_with(|requests| requests.set(requests.get().saturating_add(1)));
        }
    });
}
struct Allocator;
// SAFETY: TLS stores only scalars and every allocation is forwarded unchanged to System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        request();
        // SAFETY: Layout is the valid caller-provided layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        request();
        // SAFETY: Layout is the valid caller-provided layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        request();
        // SAFETY: The pointer, original layout and requested size are passed unchanged.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        request();
        // SAFETY: All successful pointers are original System allocations.
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
fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    ACTIVE.with(|active| assert!(!active.get()));
    REQUESTS.with(|requests| requests.set(0));
    ACTIVE.with(|active| active.set(true));
    let guard = Measurement;
    let result = operation();
    drop(guard);
    (result, REQUESTS.with(Cell::get))
}
fn calibration() {
    let ((), requests) = measure(|| {
        let mut known = Vec::<u64>::with_capacity(std::hint::black_box(4));
        known.push(std::hint::black_box(1));
        known.reserve(std::hint::black_box(128));
        drop(known);
    });
    assert!(
        requests >= 3,
        "allocation, growth and deallocation must be visible"
    );
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn owner(start: u64, frequency: u64) -> AudioAuthority {
    let basis = OutputFrameBasis::new(point(2, 0), 3, start).unwrap();
    let mut owner = AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: 8,
            max_observation_age: Duration::from_nanos(100),
            input_extrapolation: ExtrapolationPolicy::Bounded {
                before: Duration::from_nanos(2),
                after: Duration::from_nanos(8),
            },
            max_input_ahead: Duration::from_nanos(2),
        },
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.point_at_stream_frame(0).unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency },
    )
    .unwrap();
    for (position, at) in [(1, 100), (2, 101)] {
        let coordinate = NativeOutputCoordinate::Counter {
            position,
            frequency,
        };
        owner
            .observe_coordinate(
                7,
                ClockPair {
                    source: basis.point_at_native_counter(position, frequency).unwrap(),
                    target: host(at),
                },
                coordinate,
            )
            .unwrap();
    }
    owner.record_acquired_prefix(host(105)).unwrap();
    owner
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}

#[test]
fn calibrated_actual_input_frontier_commits_queries_and_value_refusals_do_not_allocate() {
    calibration();
    let mut owner = owner(0, 4_000_000_000);
    let mut merger = merger();
    let (result, requests) = measure(|| {
        let input = owner
            .prepare_coordinate_input(host(100), host(105))
            .unwrap()
            .unwrap();
        let query = input.query();
        let valid = owner.validate_coordinate_input(&input, host(105));
        let commit = owner.commit_coordinate_input(input, host(105));
        let stale = owner.commit_coordinate_input(input, host(105));
        let frontier = owner
            .prepare_coordinate_frontier(host(105), &merger)
            .unwrap()
            .unwrap();
        let ordered = frontier.query().compare_interpolated_time(query);
        let advance = frontier.advance();
        let frontier_commit = owner.commit_coordinate_frontier(frontier, host(105), &mut merger);
        let duplicate = owner
            .prepare_coordinate_frontier(host(106), &merger)
            .unwrap()
            .is_none();
        let wrong_domain = owner
            .prepare_coordinate_input(point(4, 101), host(106))
            .unwrap_err();
        (
            valid,
            commit,
            stale,
            ordered,
            advance,
            frontier_commit,
            duplicate,
            wrong_domain,
        )
    });
    assert_eq!(requests, 0);
    assert_eq!(result.0, Ok(()));
    assert_eq!(result.1, Ok(()));
    assert_eq!(result.2, Err(AudioAuthorityError::StalePreparation));
    assert_eq!(result.3, std::cmp::Ordering::Greater);
    assert_eq!(result.4, Some(point(3, 500)));
    assert_eq!(result.5, Ok(()));
    assert!(result.6);
    assert_eq!(result.7, AudioAuthorityError::DomainMismatch);
}

#[test]
fn calibrated_exact_preorigin_discard_and_classification_refusals_do_not_allocate() {
    calibration();
    let mut owner = owner(1, 3_000_000_000);
    let (result, requests) = measure(|| {
        let token = owner
            .prepare_coordinate_input(host(98), host(105))
            .unwrap()
            .unwrap();
        let before = token.is_before_origin();
        let raw = token.raw_output();
        let refused = owner.commit_coordinate_input(token, host(105));
        let discarded = owner.commit_discarded_coordinate_input(token, host(105));
        let repeated = owner.commit_discarded_coordinate_input(token, host(105));
        (
            before,
            raw,
            refused,
            discarded,
            repeated,
            owner.committed_coordinate_operation(),
            owner.committed_input_host(),
        )
    });
    assert_eq!(requests, 0);
    assert!(result.0);
    assert_eq!(result.1, point(2, 333_333_333));
    assert_eq!(result.2, Err(AudioAuthorityError::OperationRegression));
    assert_eq!(result.3, Ok(()));
    assert_eq!(result.4, Err(AudioAuthorityError::StalePreparation));
    assert!(result.5.is_none());
    assert_eq!(result.6, Some(host(98)));
}
