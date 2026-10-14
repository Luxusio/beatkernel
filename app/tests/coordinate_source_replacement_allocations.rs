//! Measures the actual successful held Source publication, prepared entirely cold.
use beatkernel_bms_runtime as runtime;
use runtime::{native_output_coordinate::NativeOutputCoordinate, playback_pause::PausePhase};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
#[path = "../src/gameplay/output/application/coordinate_source_replacement_test_support.rs"]
mod support;
use support::*;

thread_local! {
    static SOURCE_PUBLICATION_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static SOURCE_PUBLICATION_REQUESTS: Cell<usize> = const { Cell::new(0) };
    static SOURCE_PUBLICATION_FREES: Cell<usize> = const { Cell::new(0) };
}
fn record_request() {
    let _ = SOURCE_PUBLICATION_ACTIVE.try_with(|active| {
        if active.get() {
            let _ = SOURCE_PUBLICATION_REQUESTS
                .try_with(|requests| requests.set(requests.get().saturating_add(1)));
        }
    });
}
fn record_free() {
    let _ = SOURCE_PUBLICATION_ACTIVE.try_with(|active| {
        if active.get() {
            let _ =
                SOURCE_PUBLICATION_FREES.try_with(|frees| frees.set(frees.get().saturating_add(1)));
        }
    });
}
struct SourcePublicationAllocator;
// SAFETY: All operations forward the original arguments to System. TLS meters
// contain only scalar Cells, allocate nothing, and cannot unwind.
unsafe impl GlobalAlloc for SourcePublicationAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_request();
        // SAFETY: The allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_request();
        // SAFETY: The allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_request();
        // SAFETY: The caller supplies a live System allocation and valid size.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record_free();
        // SAFETY: Successful allocations remain unchanged System allocations.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static SOURCE_PUBLICATION_ALLOCATOR: SourcePublicationAllocator = SourcePublicationAllocator;
struct Meter;
impl Drop for Meter {
    fn drop(&mut self) {
        SOURCE_PUBLICATION_ACTIVE.with(|active| active.set(false));
    }
}
fn measure<T>(operation: impl FnOnce() -> T) -> (T, usize, usize) {
    SOURCE_PUBLICATION_ACTIVE.with(|active| assert!(!active.get()));
    SOURCE_PUBLICATION_REQUESTS.with(|requests| requests.set(0));
    SOURCE_PUBLICATION_FREES.with(|frees| frees.set(0));
    SOURCE_PUBLICATION_ACTIVE.with(|active| active.set(true));
    let meter = Meter;
    let result = operation();
    drop(meter);
    (
        result,
        SOURCE_PUBLICATION_REQUESTS.with(Cell::get),
        SOURCE_PUBLICATION_FREES.with(Cell::get),
    )
}

#[test]
fn coordinate_source_replacement_publication_with_original_nonempty_merger_requests_no_allocations()
{
    let (allocated, requests, _) = measure(|| Vec::<u64>::with_capacity(std::hint::black_box(128)));
    assert!(requests > 0, "meter must detect an actual allocation");
    let mut grown = Vec::<u64>::with_capacity(1);
    let ((), requests, _) = measure(|| grown.reserve_exact(std::hint::black_box(128)));
    assert!(requests > 0, "meter must detect an actual reallocation");
    let ((), requests, frees) = measure(|| drop(allocated));
    assert_eq!(requests, 0);
    assert!(
        frees > 0,
        "deallocation has an independent, explicitly reported policy"
    );
    drop(grown);

    for frequency in [None, Some(10_000_000)] {
        let mut r = Rig::with_frequency(true, frequency.map(|_| OLD_FREQUENCY));
        assert_eq!(
            r.inputs.source_count(),
            1,
            "use the original nonempty registry"
        );
        assert_eq!(
            r.inputs.pending(),
            0,
            "the acquired input prefix was actually drained"
        );
        let merger = (
            r.inputs.source_count(),
            r.inputs.source_capacity(),
            r.inputs.pending(),
        );
        let frontiers = preserved_frontiers(&r.current);
        let pin = r.pin;
        let generation = r.practice.generation();
        let (_controller, ready) = r.ready(Request::normal(frequency));
        let native_id = ready.output().native_id;
        let state = owner_facts(ready.output().source_state());
        let history = source_history(ready.output().source_state());
        let basis = ready.basis();
        let epoch = ready.pause().epoch();
        let latest = ready.snapshots()[1].coordinate();
        let calls = r.script.borrow().calls.clone();
        let stages = Cell::new(0);
        let (copied_cold_token, requests, _) =
            measure(|| std::hint::black_box(ready.prepared().clone()));
        assert!(
            requests > 0,
            "cloning the actual cold token exposes the original nonempty merger allocation"
        );
        drop(copied_cold_token);
        // Cold owner association, histories, native validation and all token
        // preparation are outside this window. It contains the real borrowed
        // authority/native/pause/current commit and one successful stage.
        let (publication, requests, _cold_storage_frees) = measure(|| {
            r.publish(ready, |_| {
                stages.set(stages.get() + 1);
                Ok(())
            })
        });
        let (hold, ()) = match publication {
            Ok(value) => value,
            Err(f) => panic!("actual publication failed: {}", f.error),
        };
        assert_eq!(
            requests, 0,
            "successful Source publication borrows the prepared merger state"
        );
        assert_eq!(stages.get(), 1);
        // Consumed cold proof storage may be freed; zero allocations/reallocations
        // is the publication contract, not a promise to suppress deallocation.
        assert_eq!(
            (
                r.inputs.source_count(),
                r.inputs.source_capacity(),
                r.inputs.pending()
            ),
            merger
        );
        assert_eq!(preserved_frontiers(&r.current), frontiers);
        assert_eq!(r.current.authority().epoch().id, epoch);
        assert_eq!(r.current.basis(), Some(basis));
        assert_eq!(r.current.practice_mixer_basis(), Some(pin));
        assert_eq!(
            r.current
                .authority()
                .latest_coordinate_observation()
                .unwrap()
                .coordinate(),
            Some(latest)
        );
        assert_eq!(r.pause.phase(), PausePhase::Paused);
        assert_eq!(r.pause.epoch(), epoch);
        assert_eq!(r.output.as_ref().unwrap().native_id, native_id);
        assert_eq!(
            owner_facts(r.output.as_ref().unwrap().source_state()),
            state
        );
        assert_eq!(
            source_history(r.output.as_ref().unwrap().source_state()),
            history
        );
        assert_eq!(r.practice.generation(), generation);
        assert_eq!(r.script.borrow().calls, calls);
        if let Some(frequency) = frequency {
            assert!(
                matches!(latest, NativeOutputCoordinate::Counter { frequency: actual, .. } if actual == frequency)
            );
        }
        assert!(r.runtime.hold_audio_pause().is_err());
        drop(hold);
        drop(r.runtime.hold_audio_pause().unwrap());
    }
}
