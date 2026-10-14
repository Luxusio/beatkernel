//! Allocation requests for actual retained coordinate publication; software evidence only.
use beatkernel_bms_runtime as runtime;
use runtime::playback_pause::PausePhase;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
#[path = "../src/gameplay/output/application/coordinate_target_replacement_test_support.rs"]
mod support;
use support::*;

thread_local! {
    static PUBLICATION_METER_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static PUBLICATION_HEAP_REQUESTS: Cell<usize> = const { Cell::new(0) };
}
fn record_publication_heap_request() {
    let _ = PUBLICATION_METER_ACTIVE.try_with(|active| {
        if active.get() {
            let _ = PUBLICATION_HEAP_REQUESTS.try_with(|requests| {
                requests.set(requests.get().saturating_add(1));
            });
        }
    });
}
struct PublicationAllocator;
// SAFETY: Every operation delegates unchanged to System. The optional TLS
// meter uses only scalar Cells, performs no allocation, and cannot panic.
unsafe impl GlobalAlloc for PublicationAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_publication_heap_request();
        // SAFETY: The allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_publication_heap_request();
        // SAFETY: The allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_publication_heap_request();
        // SAFETY: The caller supplies a live System allocation and valid size.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Successful allocations are unchanged System allocations.
        // Consumed cold token storage may be freed during publication; its
        // deallocation is deliberately outside the allocation-request count.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static PUBLICATION_ALLOCATOR: PublicationAllocator = PublicationAllocator;
struct PublicationMeasurement;
impl Drop for PublicationMeasurement {
    fn drop(&mut self) {
        PUBLICATION_METER_ACTIVE.with(|active| active.set(false));
    }
}
fn measure_publication_heap_requests<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    PUBLICATION_METER_ACTIVE.with(|active| assert!(!active.get()));
    PUBLICATION_HEAP_REQUESTS.with(|requests| requests.set(0));
    PUBLICATION_METER_ACTIVE.with(|active| active.set(true));
    let guard = PublicationMeasurement;
    let result = operation();
    drop(guard);
    (result, PUBLICATION_HEAP_REQUESTS.with(Cell::get))
}

#[test]
fn coordinate_target_replacement_successful_held_publication_with_original_registry_makes_no_heap_requests(
) {
    let (allocation, requests) =
        measure_publication_heap_requests(|| Vec::<u64>::with_capacity(std::hint::black_box(128)));
    assert!(requests > 0, "the meter must see a known allocation");
    drop(allocation);
    let mut reallocation = Vec::<u64>::with_capacity(1);
    let ((), requests) = measure_publication_heap_requests(|| {
        reallocation.reserve_exact(std::hint::black_box(128));
    });
    assert!(requests > 0, "the meter must see a known reallocation");
    drop(reallocation);

    let mut r = Rig::new(false);
    assert_eq!(
        r.inputs.source_count(),
        1,
        "the original merger registry is nonempty"
    );
    let merger = (
        r.inputs.source_count(),
        r.inputs.source_capacity(),
        r.inputs.pending(),
    );
    let frontiers = preserved_frontiers(&r.current);
    let original_source = source_history(r.output.as_ref().unwrap().state.as_ref().unwrap());
    let original_position = r
        .output
        .as_ref()
        .unwrap()
        .state
        .as_ref()
        .unwrap()
        .converter_owner()
        .source_position();
    let generation = r.practice.generation();
    let pin = r.pin;
    let (_controller, ready) = r.ready(Request::normal(32_000));
    let candidate_id = ready.output().native_id;
    let next_epoch = ready.pause().epoch();
    let next_basis = ready.basis();
    let originals = ready.snapshots();
    let native_calls = r.script.borrow().calls.clone();
    let stage_calls = Cell::new(0);
    let (cloned_preparation, requests) =
        measure_publication_heap_requests(|| std::hint::black_box(ready.prepared().clone()));
    assert!(
        requests > 0,
        "cloning this actual prepared token must expose its nonempty merger registry allocation"
    );
    drop(cloned_preparation);
    // All real converter, input-registry and opaque-token preparation is cold.
    // Only the actual successful held publication and its Ok stage are metered.
    let (publication, requests) = measure_publication_heap_requests(|| {
        r.publish(ready, |_| {
            stage_calls.set(stage_calls.get() + 1);
            Ok(())
        })
    });
    let (hold, ()) = match publication {
        Ok(value) => value,
        Err(failure) => panic!("prepared held publication failed: {}", failure.error),
    };
    assert_eq!(
        requests, 0,
        "successful publication borrows prepared merger metadata without cloning its Vec"
    );
    assert_eq!(stage_calls.get(), 1);
    assert_eq!(
        (
            r.inputs.source_count(),
            r.inputs.source_capacity(),
            r.inputs.pending()
        ),
        merger
    );
    assert_eq!(r.inputs.source_count(), 1);
    assert_eq!(r.current.authority().epoch().id, next_epoch);
    assert_eq!(r.current.target_basis(), Some(next_basis));
    assert_eq!(r.current.practice_mixer_basis(), Some(pin));
    assert_eq!(r.pause.phase(), PausePhase::Paused);
    assert_eq!(r.pause.epoch(), next_epoch);
    assert_eq!(r.output.as_ref().unwrap().native_id, candidate_id);
    assert_eq!(
        r.current
            .authority()
            .latest_coordinate_observation()
            .unwrap()
            .coordinate(),
        Some(originals[1].coordinate())
    );
    assert_eq!(preserved_frontiers(&r.current), frontiers);
    assert_eq!(
        source_history(r.output.as_ref().unwrap().state.as_ref().unwrap()),
        original_source
    );
    assert_eq!(
        r.output
            .as_ref()
            .unwrap()
            .state
            .as_ref()
            .unwrap()
            .converter_owner()
            .source_position(),
        original_position
    );
    assert_eq!(r.practice.generation(), generation);
    assert_eq!(
        r.script.borrow().calls,
        native_calls,
        "publication performs no native effect"
    );
    assert!(r.runtime.hold_audio_pause().is_err());
    drop(hold);
    let hold = r.runtime.hold_audio_pause().unwrap();
    drop(hold);
}
