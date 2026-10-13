//! Allocation requests for the real coordinate authority, not timing claims.
use beatkernel::{
    audio::OutputFrameBasis,
    time::{
        presentation::ObservationAdmission, ClockDomainId, ClockMapper, ClockPair, ClockPoint,
        Duration, ExtrapolationPolicy, Timestamp,
    },
};
use beatkernel_bms_runtime::{
    audio_authority::{
        AudioAuthority, AudioAuthorityConfig, AudioAuthorityCoordinateBasis, AudioAuthorityEpoch,
        AudioAuthorityError,
    },
    native_output_coordinate::NativeOutputCoordinate,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static REQUESTS: Cell<usize> = const { Cell::new(0) };
    static FAIL_NEXT: Cell<bool> = const { Cell::new(false) };
}
fn requested() -> bool {
    let mut refuse = false;
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            let _ = REQUESTS.try_with(|requests| requests.set(requests.get().saturating_add(1)));
            let _ = FAIL_NEXT.try_with(|fail| {
                refuse = fail.replace(false);
            });
        }
    });
    refuse
}
struct Allocator;
// SAFETY: System owns every successful allocation. TLS holds only scalars and
// never allocates or panics; refusal returns null before touching System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if requested() {
            return std::ptr::null_mut();
        }
        // SAFETY: The allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if requested() {
            return std::ptr::null_mut();
        }
        // SAFETY: The allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if requested() {
            return std::ptr::null_mut();
        }
        // SAFETY: The caller supplies a live System allocation and valid size.
        // On refusal above its original allocation remains owned by the caller.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Successful pointers are unchanged System allocations; null
        // refusals are never passed to dealloc by an allocator caller.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
struct Measurement;
impl Drop for Measurement {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
        FAIL_NEXT.with(|fail| fail.set(false));
    }
}
fn measure<T>(fail_next: bool, operation: impl FnOnce() -> T) -> (T, usize) {
    ACTIVE.with(|active| assert!(!active.get()));
    REQUESTS.with(|requests| requests.set(0));
    FAIL_NEXT.with(|fail| fail.set(fail_next));
    ACTIVE.with(|active| active.set(true));
    let guard = Measurement;
    let result = operation();
    drop(guard);
    (result, REQUESTS.with(Cell::get))
}
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn epoch() -> AudioAuthorityEpoch {
    AudioAuthorityEpoch {
        id: 7,
        stream_origin: point(2, 100),
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    }
}
fn descriptor() -> AudioAuthorityCoordinateBasis {
    AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 100), 2_000_000_000, 0).unwrap(),
        frequency: 2_000_000_000,
    }
}
fn coordinate(position: u64) -> NativeOutputCoordinate {
    NativeOutputCoordinate::Counter {
        position,
        frequency: 2_000_000_000,
    }
}
fn pair(position: u64, at: i64) -> ClockPair {
    ClockPair {
        source: point(2, 100 + (position / 2) as i64),
        target: point(1, at),
    }
}

#[test]
fn calibrated_coordinate_admission_reads_and_refusals_make_zero_heap_requests() {
    let (calibration, requests) = measure(false, || {
        Vec::<u64>::with_capacity(std::hint::black_box(128))
    });
    assert!(
        requests > 0,
        "known allocation must be visible to this meter"
    );
    drop(calibration);
    let config = AudioAuthorityConfig {
        history_capacity: 4,
        ..AudioAuthorityConfig::default()
    };
    let mut owner = AudioAuthority::new_with_coordinates(config, epoch(), descriptor()).unwrap();
    let (results, requests) = measure(false, || {
        let first = owner.observe_coordinate(7, pair(0, 100), coordinate(0));
        let second = owner.observe_coordinate(7, pair(1, 101), coordinate(1));
        let same = owner.observe_coordinate(7, pair(1, 102), coordinate(1));
        let mismatch = owner.observe_coordinate(
            7,
            pair(2, 102),
            NativeOutputCoordinate::StreamFrame { frame: 2 },
        );
        let mode = owner.observe(7, pair(2, 102));
        let latest = owner.latest_coordinate_observation();
        let basis = owner.coordinate_basis();
        let pair = owner.latest_observation();
        let prefix = owner.record_acquired_prefix(point(1, 102));
        let unavailable = owner.prepare_input(point(1, 101), point(1, 102));
        (
            first,
            second,
            same,
            mismatch,
            mode,
            latest,
            basis,
            pair,
            prefix,
            unavailable,
        )
    });
    assert_eq!(requests, 0);
    assert_eq!(results.0, Ok(ObservationAdmission::Retained));
    assert_eq!(results.1, Ok(ObservationAdmission::Retained));
    assert_eq!(results.2, Ok(ObservationAdmission::Unchanged));
    assert_eq!(results.3, Err(AudioAuthorityError::CoordinateMismatch));
    assert_eq!(results.4, Err(AudioAuthorityError::CoordinateModeMismatch));
    assert_eq!(results.5.unwrap().coordinate(), Some(coordinate(1)));
    assert_eq!(results.6, Some(descriptor()));
    assert_eq!(results.7, Some(pair(1, 101)));
    assert_eq!(results.8, Ok(()));
    assert_eq!(
        results.9,
        Err(AudioAuthorityError::CoordinateMappingUnavailable)
    );
    let (capacity, requests) = measure(false, || {
        let third = owner.observe_coordinate(7, pair(2, 102), coordinate(2));
        let fourth = owner.observe_coordinate(7, pair(3, 103), coordinate(3));
        let refusal = owner.observe_coordinate(7, pair(4, 104), coordinate(4));
        (
            third,
            fourth,
            refusal,
            owner.latest_coordinate_observation(),
        )
    });
    assert_eq!(requests, 0);
    assert_eq!(capacity.0, Ok(ObservationAdmission::Retained));
    assert_eq!(capacity.1, Ok(ObservationAdmission::Retained));
    assert_eq!(capacity.2, Err(AudioAuthorityError::HistoryCapacity));
    assert_eq!(capacity.3.unwrap().coordinate(), Some(coordinate(3)));
}

#[test]
fn failed_cold_reservation_is_recoverable_and_meter_is_reset() {
    let (failure, requests) = measure(true, || {
        AudioAuthority::new_with_coordinates(AudioAuthorityConfig::default(), epoch(), descriptor())
    });
    assert_eq!(
        requests, 1,
        "the first fallible history reservation is refused"
    );
    assert!(matches!(
        failure,
        Err(AudioAuthorityError::AllocationFailed)
    ));
    let (recovered, requests) = measure(false, || {
        AudioAuthority::new_with_coordinates(AudioAuthorityConfig::default(), epoch(), descriptor())
    });
    assert_eq!(requests, 1, "only the sole queue requires cold allocation");
    let mut recovered = recovered.unwrap();
    assert_eq!(recovered.coordinate_basis(), Some(descriptor()));
    assert_eq!(
        recovered.observe_coordinate(7, pair(0, 100), coordinate(0)),
        Ok(ObservationAdmission::Retained)
    );
    drop(recovered);
    assert_eq!(measure(false, || ()).1, 0);
}

#[test]
fn calibrated_exact_preparation_query_mapping_validation_and_refusals_do_not_allocate() {
    let (calibration, requests) = measure(false, || {
        Vec::<u64>::with_capacity(std::hint::black_box(128))
    });
    assert!(requests > 0);
    drop(calibration);
    let config = AudioAuthorityConfig {
        history_capacity: 4,
        input_extrapolation: ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(1),
            after: Duration::from_nanos(8),
        },
        max_input_ahead: Duration::from_nanos(1),
        ..AudioAuthorityConfig::default()
    };
    let mut owner = AudioAuthority::new_with_coordinates(config, epoch(), descriptor()).unwrap();
    owner
        .observe_coordinate(7, pair(0, 100), coordinate(0))
        .unwrap();
    owner
        .observe_coordinate(7, pair(1, 102), coordinate(1))
        .unwrap();
    owner.record_acquired_prefix(point(1, 110)).unwrap();
    let (result, requests) = measure(false, || {
        let token = owner
            .prepare_coordinate_input(point(1, 106), point(1, 110))
            .unwrap()
            .unwrap();
        let query = token.query().point(point(2, 100));
        let mapped = token.mapper().map(point(1, 106), ClockDomainId(3));
        let fraction_refused = token.mapper().map(point(1, 107), ClockDomainId(3));
        let validation = owner.validate_coordinate_input(&token, point(1, 111));
        let future = owner.prepare_coordinate_input(point(1, 107), point(1, 110));
        let expired = owner.prepare_coordinate_input(point(1, 98), point(1, 110));
        let wrong_domain = owner.prepare_coordinate_input(point(4, 101), point(1, 110));
        let negative = owner
            .prepare_coordinate_input(point(1, 99), point(1, 110))
            .unwrap()
            .unwrap();
        (
            token.raw_output(),
            query,
            mapped,
            fraction_refused,
            validation,
            future.unwrap().is_none(),
            expired.unwrap_err(),
            wrong_domain.unwrap_err(),
            negative.raw_output(),
        )
    });
    assert_eq!(requests, 0);
    assert_eq!(result.0, point(2, 101));
    assert_eq!(result.1.unwrap(), point(2, 101));
    assert_eq!(result.2, Some(Timestamp::from_nanos(501)));
    assert_eq!(result.3, None);
    assert_eq!(result.4, Ok(()));
    assert!(result.5);
    assert_eq!(result.6, AudioAuthorityError::HistoryExpired);
    assert_eq!(result.7, AudioAuthorityError::DomainMismatch);
    assert_eq!(result.8, point(2, 99));
}

#[test]
fn calibrated_exact_logical_overflow_and_mapper_refusal_do_not_allocate() {
    let (calibration, requests) = measure(false, || {
        Vec::<u64>::with_capacity(std::hint::black_box(128))
    });
    assert!(requests > 0);
    drop(calibration);
    let basis = AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 0), 1, 0).unwrap(),
        frequency: 1_000_000_000,
    };
    let mut owner = AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            input_extrapolation: ExtrapolationPolicy::Bounded {
                before: Duration::ZERO,
                after: Duration::from_nanos(1),
            },
            max_input_ahead: Duration::from_nanos(1),
            ..AudioAuthorityConfig::default()
        },
        AudioAuthorityEpoch {
            stream_origin: point(2, 0),
            logical_origin: point(3, i64::MAX - 2),
            ..epoch()
        },
        basis,
    )
    .unwrap();
    for (position, at) in [(0, 100), (2, 102)] {
        owner
            .observe_coordinate(
                7,
                ClockPair {
                    source: point(2, position as i64),
                    target: point(1, at),
                },
                NativeOutputCoordinate::Counter {
                    position,
                    frequency: 1_000_000_000,
                },
            )
            .unwrap();
    }
    owner.record_acquired_prefix(point(1, 103)).unwrap();
    let (result, requests) = measure(false, || {
        let token = owner
            .prepare_coordinate_input(point(1, 102), point(1, 103))
            .unwrap()
            .unwrap();
        (
            owner
                .prepare_coordinate_input(point(1, 103), point(1, 103))
                .unwrap_err(),
            token.mapper().map(point(1, 103), ClockDomainId(3)),
            owner.validate_coordinate_input(&token, point(1, 103)),
        )
    });
    assert_eq!(requests, 0);
    assert_eq!(result.0, AudioAuthorityError::Overflow);
    assert_eq!(result.1, None);
    assert_eq!(result.2, Ok(()));
}
