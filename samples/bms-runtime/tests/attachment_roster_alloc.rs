use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
use beatkernel::input::DeviceId;
use beatkernel_bms_runtime::{
    local_players::PlayerId,
    local_input::attachments::{verify, AttachmentError},
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(kind: usize) {
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
// SAFETY: All pointers/layouts forward unchanged to System; instrumentation is
// allocation-free thread-local scalar bookkeeping.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        // SAFETY: Caller supplies a live System allocation and valid new size.
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        // SAFETY: Caller supplies the original live pointer and layout.
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn lookup(attached: &[(Option<u64>, DeviceId, bool)], key: u64) -> [Option<(DeviceId, bool)>; 2] {
    let mut matches = attached
        .iter()
        .filter(|candidate| candidate.0 == Some(key))
        .map(|candidate| (candidate.1, candidate.2));
    [matches.next(), matches.next()]
}
#[test]
fn sixty_four_player_hot_roster_happy_and_refusal_paths_do_not_touch_allocator() {
    let requested: Vec<_> = (1..=64)
        .map(|id| (PlayerId(id), u64::from(id) + 1000))
        .collect();
    let selected: Vec<_> = (1..=64).map(DeviceId).collect();
    let attached: Vec<_> = requested
        .iter()
        .zip(&selected)
        .map(|((_, key), id)| (Some(*key), *id, true))
        .collect();
    let mut changed = selected.clone();
    changed[63] = DeviceId(u64::MAX);
    let mut duplicate_player = requested.clone();
    duplicate_player[63].0 = duplicate_player[62].0;
    let mut ambiguous = attached.clone();
    ambiguous.push(attached[0]);
    let mut alias = attached.clone();
    alias[63].1 = alias[62].1;
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|track| track.set(true));
    for _ in 0..1000 {
        assert_eq!(
            verify(&requested, &selected, |key| lookup(&attached, key)),
            Ok(())
        );
        assert_eq!(
            verify(&requested, &changed, |key| lookup(&attached, key)),
            Err(AttachmentError::Changed)
        );
        assert_eq!(
            verify(&duplicate_player, &selected, |key| lookup(&attached, key)),
            Err(AttachmentError::InvalidAssignment)
        );
        assert_eq!(
            verify(&requested, &selected, |key| lookup(&ambiguous, key)),
            Err(AttachmentError::Ambiguous)
        );
        assert_eq!(
            verify(&requested, &selected, |key| lookup(&alias, key)),
            Err(AttachmentError::Unusable)
        );
    }
    TRACK.with(|track| track.set(false));
    assert_eq!(COUNTS.with(Cell::get), [0; 3]);
}
