//! Thread-local System request accounting at actual public input boundaries.
//! Counts/bytes are allocator requests, not live memory, CPU time, lock/IO or
//! physical callback proof. Baselines explicitly disable processing-clock reads.
//! Caller-supplied mapper/evaluator work is included. Fixture preparation,
//! assertions, telemetry summaries, logging and returned-report drops are outside.
use beatkernel::{
    audio::{command_queue, AudioCommand, CommandConsumer, QueuePushError, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::{InputOwner, InstantEvaluator, InteractionState},
    judge::*,
    runtime::{Runtime, RuntimeError, RuntimeProcessingClock, RuntimeReport, SoundBinding},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_bms_runtime::{
    local_players::PlayerId,
    local_runtime::{FailureKind, InputResult, MemberConfig, RuntimeGroup, SoloRuntime},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Operation {
    attempted: usize,
    succeeded: usize,
    requested_bytes: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Requests {
    alloc: Operation,
    zeroed: Operation,
    realloc: Operation,
    dealloc: Operation,
    realloc_old_bytes: usize,
    overflow: bool,
}
#[derive(Clone, Copy, Default)]
struct MeterState {
    active: bool,
    requests: Requests,
}
thread_local! {
    static METER: Cell<MeterState> = const { Cell::new(MeterState {
        active: false,
        requests: Requests {
            alloc: Operation { attempted: 0, succeeded: 0, requested_bytes: 0 },
            zeroed: Operation { attempted: 0, succeeded: 0, requested_bytes: 0 },
            realloc: Operation { attempted: 0, succeeded: 0, requested_bytes: 0 },
            dealloc: Operation { attempted: 0, succeeded: 0, requested_bytes: 0 },
            realloc_old_bytes: 0, overflow: false,
        },
    }) };
}
#[derive(Clone, Copy)]
enum Hook {
    Alloc,
    Zeroed,
    Realloc,
    Dealloc,
}
fn add(value: &mut usize, amount: usize, overflow: &mut bool) {
    match value.checked_add(amount) {
        Some(next) => *value = next,
        None => *overflow = true,
    }
}
fn record(hook: Hook, requested: usize, old: usize, succeeded: bool) {
    // No formatting, assertions, locks, references to allocation contents, or
    // fallible TLS initialization. A dying thread still forwards System calls;
    // no measurement can start or finish when its TLS is unavailable.
    let _ = METER.try_with(|slot| {
        let mut state = slot.get();
        if !state.active {
            return;
        }
        let stats = &mut state.requests;
        let op = match hook {
            Hook::Alloc => &mut stats.alloc,
            Hook::Zeroed => &mut stats.zeroed,
            Hook::Realloc => &mut stats.realloc,
            Hook::Dealloc => &mut stats.dealloc,
        };
        add(&mut op.attempted, 1, &mut stats.overflow);
        add(
            &mut op.succeeded,
            usize::from(succeeded),
            &mut stats.overflow,
        );
        add(&mut op.requested_bytes, requested, &mut stats.overflow);
        if matches!(hook, Hook::Realloc) {
            add(&mut stats.realloc_old_bytes, old, &mut stats.overflow);
        }
        slot.set(state);
    });
}
struct Allocator;
// SAFETY: System receives every pointer, layout and new size unchanged. Hooks
// retain only scalar TLS counters. Successful means nonnull for allocation and
// realloc, and a forwarded call for dealloc (which has no failure return).
// Requested bytes count attempts, including a hypothetical null return; realloc
// old-layout bytes are separately counted. No bytes are called live/peak/RSS.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        record(Hook::Alloc, layout.size(), 0, !pointer.is_null());
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record(Hook::Zeroed, layout.size(), 0, !pointer.is_null());
        pointer
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        record(Hook::Realloc, size, layout.size(), !result.is_null());
        result
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        record(Hook::Dealloc, layout.size(), 0, true);
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
struct Measurement(std::marker::PhantomData<*mut ()>);
impl Measurement {
    fn begin() -> Result<Self, &'static str> {
        METER.with(|slot| {
            if slot.get().active {
                return Err("nested allocation measurement");
            }
            slot.set(MeterState {
                active: true,
                requests: Requests::default(),
            });
            Ok(Self(std::marker::PhantomData))
        })
    }
    fn finish(self) -> Requests {
        let stats = METER.with(|slot| {
            let mut state = slot.get();
            state.active = false;
            slot.set(state);
            state.requests
        });
        drop(self);
        assert!(!stats.overflow, "allocator request counter overflow");
        stats
    }
}
impl Drop for Measurement {
    fn drop(&mut self) {
        let _ = METER.try_with(|slot| {
            let mut state = slot.get();
            state.active = false;
            slot.set(state);
        });
    }
}
fn measure<T>(operation: impl FnOnce() -> T) -> (T, Requests) {
    let guard = Measurement::begin().expect("measurement must not nest");
    let result = operation();
    (result, guard.finish())
}
// Initially frozen on 2026-10-16 from the root's actual debug and release
// runs of meter source SHA256
// 30b4854e19bd3ddcb3ae8d9a0f4331f8efccfea607cd425377cf12736c986093.
// Original logs: target/wf/runtime-input-alloc-ac002-14124/
// baseline-{debug,release}-calibrated.log (ignored diagnostic artifacts).
// Historical provenance: doc/changes/2026-10-16-runtime-input-allocations.md.
// After Judge's empty-route optimization, only the 31 measured lower-cost cases
// were tightened from actual debug/release runs of the unchanged pre-budget
// meter SHA256 33b57d357cf710fbaf60921886b16dd4083c442e49a2cc0410e13c26e9d858d6
// and engine.rs SHA256
// 1b70cc1f73423b97b226d231a4c55bb67fb3ed25900375e83a972b83784fdd15.
// Post-route logs: target/wf/judge-refresh-ac002-14125/before-budgets/
// meter-{debug,release}.log; full comparison: post-route-measurements.json.
// Of those cases, 30 save one allocation/deallocation request and four bytes
// each; runtime.observed.fanout saves two requests and eight bytes each.
// The other 35 cases and all workloads are unchanged; zeroed/realloc fields,
// realloc_old_bytes and overflow are unchanged in every case.
// Route provenance: doc/changes/2026-10-16-judge-empty-route-allocation.md.
// After Runtime's first owned judgment batch reuse, only 26 measured lower-cost
// cases were tightened from actual debug/release runs of the unchanged meter
// SHA256 9f99bcef5e22021180a5702ef2dd7a4c0304bf1fd443d9b0c6942012929fb305
// and runtime/mod.rs SHA256
// 13f8b9fd4fb6c6ff3d1f37e1b79d0dcd6bfad137edc20e639ef9be520b31f66e.
// Post-batch logs: target/wf/runtime-first-batch-ac002-14126/before-budgets/
// meter-{debug,release}.log; full comparison: post-batch-measurements.json.
// Each changed case saves one allocation/deallocation request and 544 requested
// bytes each; the other 40 cases, including direct Judge and calibration rows,
// are unchanged. All workloads, zeroed/realloc fields, realloc_old_bytes and
// overflow are unchanged. Returned Vec capacity is measured separately; request
// savings do not establish a retained/live-memory or RSS reduction.
// Current provenance: doc/changes/2026-10-16-runtime-first-judge-batch-reuse.md.
// Measured environment: rustc 1.98.1 (48a229cea), LLVM22.1.8, Linux x86_64,
// pointer8 / PhysicalInputEvent144 / JudgeEvent136 / RuntimeReport344 bytes.
// Limits are empirical allocator request ceilings, not portable memory-layout
// promises or CPU/lock/IO/physical-callback guarantees. No compiler-version or
// current type-size escape hatch: future supported-host builds check these same
// numbers. Other targets keep every workload/oracle/calibration and emit an
// explicit unverified-target budget marker, without scaling the byte ceilings.
const EMPIRICAL_BUDGET_TARGET: bool = cfg!(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_pointer_width = "64"
));
fn frozen_budget(name: &str) -> Option<Requests> {
    Some(match name {
        "calibration.alloc" => Requests {
            alloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 37,
            },
            ..Requests::default()
        },
        "calibration.dealloc" => Requests {
            dealloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 113,
            },
            ..Requests::default()
        },
        "calibration.nested" => Requests {
            alloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 29,
            },
            ..Requests::default()
        },
        "calibration.noop"
        | "calibration.thread-parent"
        | "group.1.unassigned"
        | "group.2.unassigned"
        | "group.4.unassigned"
        | "group.64.unassigned"
        | "group.failure.core-domain"
        | "group.poisoned.core-domain"
        | "group.poisoned.disconnected"
        | "group.poisoned.full"
        | "judge.observed.held"
        | "judge.observed.repeat"
        | "judge.observed.time-regression"
        | "judge.observed.up"
        | "runtime.external.refused"
        | "runtime.refused.audio-domain"
        | "runtime.refused.host-time"
        | "runtime.refused.input-domain"
        | "runtime.refused.sequence"
        | "solo.poisoned.disconnected"
        | "solo.poisoned.full" => Requests::default(),
        "calibration.realloc" => Requests {
            realloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 113,
            },
            realloc_old_bytes: 37,
            ..Requests::default()
        },
        "calibration.returned" => Requests {
            alloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 83,
            },
            ..Requests::default()
        },
        "calibration.thread-child" => Requests {
            alloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 97,
            },
            ..Requests::default()
        },
        "calibration.zeroed" => Requests {
            zeroed: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 61,
            },
            ..Requests::default()
        },
        "group.1.automatic" | "group.1.first" | "group.1.last" | "group.2.first"
        | "group.2.last" | "group.4.first" | "group.4.last" | "group.64.first"
        | "group.64.last" => Requests {
            alloc: Operation {
                attempted: 9,
                succeeded: 9,
                requested_bytes: 2176,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "group.64.raw-hid.consumed" => Requests {
            dealloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 257,
            },
            ..Requests::default()
        },
        "group.failure.disconnected" | "group.failure.full" => Requests {
            alloc: Operation {
                attempted: 9,
                succeeded: 9,
                requested_bytes: 2208,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "judge.association.reused" | "judge.prefix.4096" => Requests {
            alloc: Operation {
                attempted: 4,
                succeeded: 4,
                requested_bytes: 888,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "judge.legacy.hit.fresh" | "judge.observed.hit.fresh" | "judge.prefix.0" => Requests {
            alloc: Operation {
                attempted: 5,
                succeeded: 5,
                requested_bytes: 1004,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "judge.observed.unmatched" => Requests {
            alloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 116,
            },
            ..Requests::default()
        },
        "runtime.association.reused" | "runtime.prefix.4096" => Requests {
            alloc: Operation {
                attempted: 6,
                succeeded: 6,
                requested_bytes: 1752,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "runtime.audio.admitted"
        | "runtime.external.accepted"
        | "runtime.mapper.same-domain-control"
        | "runtime.observed.hit.fresh" => Requests {
            alloc: Operation {
                attempted: 9,
                succeeded: 9,
                requested_bytes: 2080,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "runtime.audio.disconnected" | "runtime.audio.full" => Requests {
            alloc: Operation {
                attempted: 9,
                succeeded: 9,
                requested_bytes: 2112,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "runtime.legacy.hit.fresh" => Requests {
            alloc: Operation {
                attempted: 8,
                succeeded: 8,
                requested_bytes: 1824,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "runtime.mapper.allocating" => Requests {
            alloc: Operation {
                attempted: 11,
                succeeded: 11,
                requested_bytes: 2142,
            },
            dealloc: Operation {
                attempted: 5,
                succeeded: 5,
                requested_bytes: 406,
            },
            ..Requests::default()
        },
        "runtime.observed.fanout" => Requests {
            alloc: Operation {
                attempted: 13,
                succeeded: 13,
                requested_bytes: 2968,
            },
            dealloc: Operation {
                attempted: 7,
                succeeded: 7,
                requested_bytes: 1232,
            },
            ..Requests::default()
        },
        "runtime.observed.held" | "runtime.observed.repeat" | "runtime.observed.up" => Requests {
            alloc: Operation {
                attempted: 2,
                succeeded: 2,
                requested_bytes: 864,
            },
            ..Requests::default()
        },
        "runtime.observed.unbound" | "runtime.raw-hid.returned" => Requests {
            alloc: Operation {
                attempted: 1,
                succeeded: 1,
                requested_bytes: 84,
            },
            ..Requests::default()
        },
        "runtime.prefix.0" => Requests {
            alloc: Operation {
                attempted: 8,
                succeeded: 8,
                requested_bytes: 1952,
            },
            dealloc: Operation {
                attempted: 3,
                succeeded: 3,
                requested_bytes: 344,
            },
            ..Requests::default()
        },
        "solo.audio.admitted" => Requests {
            alloc: Operation {
                attempted: 9,
                succeeded: 9,
                requested_bytes: 2176,
            },
            dealloc: Operation {
                attempted: 4,
                succeeded: 4,
                requested_bytes: 696,
            },
            ..Requests::default()
        },
        "solo.audio.disconnected" | "solo.audio.full" => Requests {
            alloc: Operation {
                attempted: 9,
                succeeded: 9,
                requested_bytes: 2208,
            },
            dealloc: Operation {
                attempted: 4,
                succeeded: 4,
                requested_bytes: 696,
            },
            ..Requests::default()
        },
        _ => return None,
    })
}

fn within_budget(actual: Requests, limit: Requests) -> Result<(), &'static str> {
    // Compare each operation and each unit independently. Counts do not stand
    // in for bytes; successful calls do not stand in for attempted calls.
    for (field, observed, ceiling) in [
        (
            "alloc.attempted",
            actual.alloc.attempted,
            limit.alloc.attempted,
        ),
        (
            "alloc.succeeded",
            actual.alloc.succeeded,
            limit.alloc.succeeded,
        ),
        (
            "alloc.requested_bytes",
            actual.alloc.requested_bytes,
            limit.alloc.requested_bytes,
        ),
        (
            "zeroed.attempted",
            actual.zeroed.attempted,
            limit.zeroed.attempted,
        ),
        (
            "zeroed.succeeded",
            actual.zeroed.succeeded,
            limit.zeroed.succeeded,
        ),
        (
            "zeroed.requested_bytes",
            actual.zeroed.requested_bytes,
            limit.zeroed.requested_bytes,
        ),
        (
            "realloc.attempted",
            actual.realloc.attempted,
            limit.realloc.attempted,
        ),
        (
            "realloc.succeeded",
            actual.realloc.succeeded,
            limit.realloc.succeeded,
        ),
        (
            "realloc.requested_bytes",
            actual.realloc.requested_bytes,
            limit.realloc.requested_bytes,
        ),
        (
            "dealloc.attempted",
            actual.dealloc.attempted,
            limit.dealloc.attempted,
        ),
        (
            "dealloc.succeeded",
            actual.dealloc.succeeded,
            limit.dealloc.succeeded,
        ),
        (
            "dealloc.requested_bytes",
            actual.dealloc.requested_bytes,
            limit.dealloc.requested_bytes,
        ),
        (
            "realloc_old_bytes",
            actual.realloc_old_bytes,
            limit.realloc_old_bytes,
        ),
    ] {
        if observed > ceiling {
            return Err(field);
        }
    }
    // Overflow is an invalid measurement even if every retained scalar fits.
    if actual.overflow || limit.overflow {
        return Err("overflow");
    }
    Ok(())
}

#[test]
fn frozen_budget_rejects_unknown_cases_and_individual_exceeded_fields() {
    assert!(frozen_budget("runtime.unknown-case").is_none());
    let limit = frozen_budget("runtime.observed.hit.fresh").unwrap();
    // These are validator negative controls, not synthetic workload results.
    // Each copy changes exactly one scalar, preventing an unrelated exceeded
    // field from masking a missing check. They never emit public-call evidence.
    for field in [
        "alloc.attempted",
        "alloc.succeeded",
        "alloc.requested_bytes",
        "zeroed.attempted",
        "zeroed.succeeded",
        "zeroed.requested_bytes",
        "realloc.attempted",
        "realloc.succeeded",
        "realloc.requested_bytes",
        "dealloc.attempted",
        "dealloc.succeeded",
        "dealloc.requested_bytes",
        "realloc_old_bytes",
        "overflow",
    ] {
        let mut exceeded = limit;
        match field {
            "alloc.attempted" => exceeded.alloc.attempted += 1,
            "alloc.succeeded" => exceeded.alloc.succeeded += 1,
            "alloc.requested_bytes" => exceeded.alloc.requested_bytes += 1,
            "zeroed.attempted" => exceeded.zeroed.attempted += 1,
            "zeroed.succeeded" => exceeded.zeroed.succeeded += 1,
            "zeroed.requested_bytes" => exceeded.zeroed.requested_bytes += 1,
            "realloc.attempted" => exceeded.realloc.attempted += 1,
            "realloc.succeeded" => exceeded.realloc.succeeded += 1,
            "realloc.requested_bytes" => exceeded.realloc.requested_bytes += 1,
            "dealloc.attempted" => exceeded.dealloc.attempted += 1,
            "dealloc.succeeded" => exceeded.dealloc.succeeded += 1,
            "dealloc.requested_bytes" => exceeded.dealloc.requested_bytes += 1,
            "realloc_old_bytes" => exceeded.realloc_old_bytes += 1,
            "overflow" => exceeded.overflow = true,
            _ => unreachable!(),
        }
        assert_eq!(within_budget(exceeded, limit), Err(field));
    }
}

fn raw(name: &str, parameters: &str, stats: Requests) {
    assert!(!stats.overflow);
    println!("INPUT_ALLOC case={name} params={parameters} profile={} arch={} os={} pointer_bytes={} event_bytes={} judge_event_bytes={} runtime_report_bytes={} requests={stats:?}",
        if cfg!(debug_assertions) { "debug" } else { "release" },
        std::env::consts::ARCH, std::env::consts::OS,
        std::mem::size_of::<usize>(), std::mem::size_of::<PhysicalInputEvent>(),
        std::mem::size_of::<JudgeEvent>(), std::mem::size_of::<RuntimeReport>());
    // Lookup is unconditional: newly named or misspelled workloads must never
    // silently receive a default/global budget, including on unverified targets.
    let limit =
        frozen_budget(name).unwrap_or_else(|| panic!("unknown input allocation case: {name}"));
    if EMPIRICAL_BUDGET_TARGET {
        if let Err(field) = within_budget(stats, limit) {
            panic!("input allocation budget exceeded: case={name} field={field} actual={stats:?} frozen={limit:?}");
        }
        println!("INPUT_ALLOC_BUDGET case={name} budget=checked-linux-x86_64-64bit");
    } else {
        println!("INPUT_ALLOC_BUDGET case={name} budget=unverified-target");
    }
}
fn one(bytes: usize) -> Operation {
    Operation {
        attempted: 1,
        succeeded: 1,
        requested_bytes: bytes,
    }
}

#[test]
fn calibration_all_four_hooks_preserve_contents_and_exact_request_units() {
    let layout = Layout::from_size_align(37, 8).unwrap();
    let (pointer, stats) = measure(|| unsafe { std::alloc::alloc(black_box(layout)) });
    assert!(!pointer.is_null());
    assert_eq!(pointer as usize % layout.align(), 0);
    assert_eq!(
        stats,
        Requests {
            alloc: one(37),
            ..Requests::default()
        }
    );
    // SAFETY: Nonnull, correctly aligned live allocation with 37 valid bytes.
    unsafe {
        for i in 0..37 {
            pointer.add(i).write_volatile(i as u8 ^ 0x5a);
        }
    }
    raw("calibration.alloc", "layout=37/8", stats);
    let (grown, stats) = measure(|| unsafe {
        std::alloc::realloc(black_box(pointer), black_box(layout), black_box(113))
    });
    assert!(!grown.is_null());
    assert_eq!(grown as usize % layout.align(), 0);
    assert_eq!(
        stats,
        Requests {
            realloc: one(113),
            realloc_old_bytes: 37,
            ..Requests::default()
        }
    );
    unsafe {
        for i in 0..37 {
            assert_eq!(grown.add(i).read_volatile(), i as u8 ^ 0x5a);
        }
    }
    let grown_layout = Layout::from_size_align(113, 8).unwrap();
    unsafe {
        grown.add(112).write_volatile(0x7b);
        assert_eq!(grown.add(112).read_volatile(), 0x7b);
    }
    raw("calibration.realloc", "old=37/8 new=113", stats);
    let ((), stats) =
        measure(|| unsafe { std::alloc::dealloc(black_box(grown), black_box(grown_layout)) });
    assert_eq!(
        stats,
        Requests {
            dealloc: one(113),
            ..Requests::default()
        }
    );
    raw("calibration.dealloc", "layout=113/8", stats);
    let zero_layout = Layout::from_size_align(61, 16).unwrap();
    let (zero, stats) = measure(|| unsafe { std::alloc::alloc_zeroed(black_box(zero_layout)) });
    assert!(!zero.is_null());
    assert_eq!(zero as usize % zero_layout.align(), 0);
    assert_eq!(
        stats,
        Requests {
            zeroed: one(61),
            ..Requests::default()
        }
    );
    unsafe {
        for i in 0..61 {
            assert_eq!(zero.add(i).read_volatile(), 0);
        }
        zero.add(60).write_volatile(0x33);
        assert_eq!(zero.add(60).read_volatile(), 0x33);
        std::alloc::dealloc(zero, zero_layout);
    }
    raw("calibration.zeroed", "layout=61/16", stats);
}

#[test]
fn calibration_noop_return_drop_nested_unwind_and_thread_isolation() {
    let (_, stats) = measure(|| black_box(()));
    assert_eq!(stats, Requests::default());
    raw("calibration.noop", "clock=none", stats);
    let (returned, stats) = measure(|| black_box(vec![black_box(0x35_u8); 83]));
    assert_eq!(returned, vec![0x35; 83]);
    assert_eq!(
        stats,
        Requests {
            alloc: one(83),
            ..Requests::default()
        }
    );
    raw("calibration.returned", "83 bytes drop outside", stats);
    drop(returned);
    let inactive_before = METER.with(|slot| slot.get().requests);
    let inactive = black_box(vec![black_box(0x12_u8); 101]);
    assert_eq!(inactive[100], 0x12);
    drop(inactive);
    assert!(!METER.with(|slot| slot.get().active));
    assert_eq!(METER.with(|slot| slot.get().requests), inactive_before);
    let guard = Measurement::begin().unwrap();
    assert!(Measurement::begin().is_err());
    let layout = Layout::from_size_align(29, 1).unwrap();
    let pointer = black_box(unsafe { std::alloc::alloc(black_box(layout)) });
    let stats = guard.finish();
    assert!(!pointer.is_null());
    // SAFETY: The nonnull System allocation owns all 29 bytes until the
    // unmeasured dealloc below. Opaque pointer escape and volatile contents
    // keep this positive control observable in optimized release builds.
    unsafe {
        pointer.write_volatile(0x3b);
        pointer.add(28).write_volatile(0x5c);
        assert_eq!(pointer.read_volatile(), 0x3b);
        assert_eq!(pointer.add(28).read_volatile(), 0x5c);
    }
    assert_eq!(
        stats,
        Requests {
            alloc: one(29),
            ..Requests::default()
        }
    );
    unsafe {
        std::alloc::dealloc(pointer, layout);
    }
    raw(
        "calibration.nested",
        "rejected inner preserves outer",
        stats,
    );
    let panic = std::panic::catch_unwind(|| {
        let _guard = Measurement::begin().unwrap();
        panic!("calibration unwind");
    });
    assert!(panic.is_err());
    assert!(!METER.with(|slot| slot.get().active));
    assert_eq!(measure(|| ()).1, Requests::default());
    // Thread construction, barriers and joining stay outside the active scope.
    // Atomic handshakes ask an already-live thread to allocate while this
    // thread measures only scalar loads; that thread's meter is independent.
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let gate = Arc::new(AtomicUsize::new(0));
    let child_gate = Arc::clone(&gate);
    let child = std::thread::spawn(move || {
        while child_gate.load(Ordering::Acquire) != 1 {
            std::hint::spin_loop();
        }
        let (data, counts) = measure(|| black_box(vec![black_box(0x7d_u8); 97]));
        child_gate.store(2, Ordering::Release);
        (data, counts)
    });
    let guard = Measurement::begin().unwrap();
    gate.store(1, Ordering::Release);
    while gate.load(Ordering::Acquire) != 2 {
        std::hint::spin_loop();
    }
    let parent_stats = guard.finish();
    let (data, child_stats) = child.join().unwrap();
    assert_eq!(data, vec![0x7d; 97]);
    assert_eq!(parent_stats, Requests::default());
    assert_eq!(
        child_stats,
        Requests {
            alloc: one(97),
            ..Requests::default()
        }
    );
    raw(
        "calibration.thread-parent",
        "other thread allocates97",
        parent_stats,
    );
    raw(
        "calibration.thread-child",
        "own thread allocates97",
        child_stats,
    );
}

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn point(domain: u32, n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(n),
    }
}
fn physical() -> PhysicalControlId {
    PhysicalControlId::keyboard(4)
}
fn button(source: u64, at: i64, seq: u64, state: ButtonState) -> PhysicalInputEvent {
    let mut meta = EventMeta::new(DeviceId(source), point(1, 1000 + at), seq);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(17),
        code: Some(4),
        timestamp: Some(point(9, at - 7)),
    });
    PhysicalInputEvent::Button(ButtonEvent {
        meta,
        control: physical(),
        state,
    })
}
fn bound(input: PhysicalInputEvent, control: u32) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: input,
    }
}
fn owner(source: u64, control: u32) -> InputOwner {
    InputOwner {
        source: DeviceId(source),
        physical: physical(),
        game_control: GameControlId(control),
    }
}
struct NoMapping;
impl ClockMapper for NoMapping {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Unknown
    }
}
fn judge(notes: &[(u64, i64, u32)]) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = notes
        .iter()
        .map(|&(id, at, _)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(at).unwrap(),
            end: None,
            interaction: InteractionId(id as u32),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    JudgeEngine::new(
        source.compile().unwrap(),
        notes
            .iter()
            .map(|&(id, _, control)| Rule {
                interaction: InteractionId(id as u32),
                control: GameControlId(control),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}
fn hit(id: u64, at: i64, input: &PhysicalInputEvent) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(id),
        stage: JudgeStage::Instant,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(7),
            delta: Duration::ZERO,
        },
        at: ts(at),
        input: Some(*input.meta()),
    }
}
fn disposition(
    d: InputDisposition,
    freshness: InputFreshness,
    selected: Option<u64>,
    results: usize,
) {
    assert_eq!(d.freshness(), freshness);
    assert_eq!(d.selected(), selected.map(ObjectId));
    assert_eq!(d.candidate_count(), usize::from(selected.is_some()));
    assert_eq!(d.dispatched_count(), usize::from(selected.is_some()));
    assert_eq!(d.input_result_count(), results);
    assert_eq!(d.passive_result_count(), 0);
    assert_eq!(d.input_hazard_count(), 0);
    assert_eq!(
        d.unmatched_fresh_press(),
        freshness == InputFreshness::FreshPress && selected.is_none()
    );
}
fn bindings(controls: &[u32], device: DeviceSelector) -> BindingMap {
    BindingMap::from_bindings(controls.iter().map(|&control| Binding {
        device,
        physical: physical(),
        game_control: GameControlId(control),
    }))
    .unwrap()
}
fn sound(id: u64, voice: u64) -> SoundBinding {
    SoundBinding {
        object: ObjectId(id),
        stage: JudgeStage::Instant,
        sample: SampleId(3),
        voice: VoiceId(voice),
        gain: 0.5,
    }
}
fn command(voice: u64) -> AudioCommand {
    AudioCommand::Play {
        voice: VoiceId(voice),
        sample: SampleId(3),
        at: ts(9000),
        gain: 0.5,
    }
}
fn sentinel() -> AudioCommand {
    AudioCommand::Stop {
        voice: VoiceId(777),
        at: ts(8888),
    }
}
fn make_runtime(
    judge: JudgeEngine,
    controls: &[u32],
    sounds: Vec<SoundBinding>,
    capacity: usize,
) -> (Runtime, CommandConsumer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut runtime = Runtime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(1000), ts(0), Rate::NORMAL),
        bindings(controls, DeviceSelector::Any),
        judge,
        producer,
        sounds,
        8,
    )
    .unwrap();
    runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
    (runtime, consumer)
}
fn report(
    report: &RuntimeReport,
    input: &PhysicalInputEvent,
    controls: &[u32],
    expected_events: &[JudgeEvent],
    at: i64,
) {
    assert_eq!(report.input.as_ref(), Some(input));
    assert_eq!(
        report.bound_inputs,
        controls
            .iter()
            .map(|&c| bound(input.clone(), c))
            .collect::<Vec<_>>()
    );
    assert_eq!(report.song_time, ts(at));
    assert_eq!(report.audio_at, point(2, 9000));
    assert_eq!(report.input_mapping_quality, ClockMappingQuality::Exact);
    assert_eq!(report.audio_mapping_quality, ClockMappingQuality::Exact);
    assert!(!report.song_end_reached);
    assert_eq!(report.judge_events, expected_events);
    assert!(report.hazard_events.is_empty());
    assert_eq!(report.judge_error, None);
}
fn no_audio(report: &RuntimeReport) {
    assert!(report.audio_commands.is_empty());
    assert!(report.audio_failures.is_empty());
}
fn queue_empty(consumer: &mut CommandConsumer) {
    assert!(consumer.try_pop().is_err());
}

#[test]
fn judge_legacy_observed_and_input_freshness_costs_keep_exact_semantics() {
    for observed in [false, true] {
        let mut engine = judge(&[(17, 100, 1)]);
        let input = bound(button(55, 100, 91, ButtonState::Down), 1);
        if observed {
            let (result, stats) =
                measure(|| engine.push_input_report(black_box(&input), black_box(ts(100))));
            let result = result.unwrap();
            assert_eq!(result.events, [hit(17, 100, &input.physical)]);
            disposition(result.disposition, InputFreshness::FreshPress, Some(17), 1);
            raw(
                "judge.observed.hit.fresh",
                "notes=1 prefix=0 source=55",
                stats,
            );
        } else {
            let (result, stats) =
                measure(|| engine.push_input(black_box(&input), black_box(ts(100))));
            assert_eq!(result.unwrap(), [hit(17, 100, &input.physical)]);
            raw(
                "judge.legacy.hit.fresh",
                "notes=1 prefix=0 source=55",
                stats,
            );
        }
        assert_eq!(
            engine.state(ObjectId(17)),
            Some(InteractionState::Completed)
        );
        assert_eq!(engine.effective_song_time(), Some(ts(100)));
        assert!(engine.is_held(owner(55, 1)));
    }
    for (name, state, warm, freshness, held) in [
        (
            "judge.observed.unmatched",
            ButtonState::Down,
            false,
            InputFreshness::FreshPress,
            true,
        ),
        (
            "judge.observed.held",
            ButtonState::Down,
            true,
            InputFreshness::HeldDown,
            true,
        ),
        (
            "judge.observed.repeat",
            ButtonState::Repeat,
            true,
            InputFreshness::ExplicitRepeat,
            true,
        ),
        (
            "judge.observed.up",
            ButtonState::Up,
            true,
            InputFreshness::Other,
            false,
        ),
    ] {
        let mut engine = judge(&[]);
        if warm {
            let first = bound(button(55, 100, 90, ButtonState::Down), 1);
            let result = engine.push_input_report(&first, ts(100)).unwrap();
            disposition(result.disposition, InputFreshness::FreshPress, None, 0);
            assert!(result.events.is_empty());
            assert!(engine.is_held(owner(55, 1)));
        }
        let input = bound(button(55, 100, 91, state), 1);
        let (result, stats) =
            measure(|| engine.push_input_report(black_box(&input), black_box(ts(100))));
        let result = result.unwrap();
        assert!(result.events.is_empty());
        disposition(result.disposition, freshness, None, 0);
        assert_eq!(engine.is_held(owner(55, 1)), held);
        assert_eq!(engine.effective_song_time(), Some(ts(100)));
        raw(name, "notes=0 source=55", stats);
    }
    let mut engine = judge(&[(17, 100, 1)]);
    let input = bound(button(55, 100, 91, ButtonState::Down), 1);
    assert_eq!(
        engine.push_input(&input, ts(100)).unwrap(),
        [hit(17, 100, &input.physical)]
    );
    let hash = engine.stable_hash().unwrap();
    let (result, stats) =
        measure(|| engine.push_input_report(black_box(&input), black_box(ts(99))));
    assert_eq!(result, Err(JudgeError::NonMonotonicSongTime));
    assert_eq!(engine.stable_hash().unwrap(), hash);
    assert_eq!(engine.effective_song_time(), Some(ts(100)));
    assert_eq!(
        engine.state(ObjectId(17)),
        Some(InteractionState::Completed)
    );
    assert!(engine.is_held(owner(55, 1)));
    raw(
        "judge.observed.time-regression",
        "100->99 unchanged-state",
        stats,
    );
}

#[test]
fn calibration_counter_overflow_is_rejected_without_disabling_system_forwarding() {
    let layout = Layout::from_size_align(19, 1).unwrap();
    let guard = Measurement::begin().unwrap();
    METER.with(|slot| {
        let mut state = slot.get();
        state.requests.alloc.attempted = usize::MAX;
        slot.set(state);
    });
    let pointer = unsafe { std::alloc::alloc(black_box(layout)) };
    // Finish turns the meter off before rejecting overflow. The allocation was
    // still forwarded, is observed, and is disposed after the rejected scope.
    let failure = std::panic::catch_unwind(|| guard.finish());
    assert!(failure.is_err());
    assert!(!pointer.is_null());
    unsafe {
        pointer.add(18).write_volatile(0x4c);
        assert_eq!(pointer.add(18).read_volatile(), 0x4c);
        std::alloc::dealloc(pointer, layout);
    }
    assert!(!METER.with(|slot| slot.get().active));
    assert_eq!(measure(|| ()).1, Requests::default());
}

#[test]
fn actual_completed_judgment_prefix_and_reused_association_are_measured() {
    // JudgeEngine stores interaction states, not an accumulated history Vec.
    // Every prefix judgment is executed and checked before its report is dropped.
    for prefix in [0_usize, 1, 4096] {
        let notes: Vec<_> = (1..=prefix + 1)
            .map(|id| (id as u64, id as i64 * 100, 1))
            .collect();
        let mut engine = judge(&notes);
        for id in 1..=prefix {
            let at = id as i64 * 100;
            let down = bound(button(55, at, id as u64 * 2 - 1, ButtonState::Down), 1);
            assert_eq!(
                engine.push_input(&down, ts(at)).unwrap(),
                [hit(id as u64, at, &down.physical)]
            );
            let up = bound(button(55, at, id as u64 * 2, ButtonState::Up), 1);
            assert!(engine.push_input(&up, ts(at)).unwrap().is_empty());
            assert_eq!(
                engine.state(ObjectId(id as u64)),
                Some(InteractionState::Completed)
            );
        }
        let next = prefix as u64 + 1;
        assert_eq!(engine.chart().objects().len(), prefix + 1);
        assert_eq!(
            engine.state(ObjectId(next)),
            Some(InteractionState::Pending)
        );
        assert!(!engine.is_held(owner(55, 1)));
        let before = engine.stable_hash().unwrap();
        let at = next as i64 * 100;
        let input = bound(button(55, at, next * 2 - 1, ButtonState::Down), 1);
        let (result, stats) =
            measure(|| engine.push_input_report(black_box(&input), black_box(ts(at))));
        let result = result.unwrap();
        assert_eq!(result.events, [hit(next, at, &input.physical)]);
        disposition(
            result.disposition,
            InputFreshness::FreshPress,
            Some(next),
            1,
        );
        assert_eq!(
            engine.state(ObjectId(next)),
            Some(InteractionState::Completed)
        );
        assert!(engine.is_held(owner(55, 1)));
        assert_ne!(engine.stable_hash().unwrap(), before);
        raw(
            match prefix {
                0 => "judge.prefix.0",
                1 => "judge.association.reused",
                _ => "judge.prefix.4096",
            },
            &format!("chart={} completed_prefix={prefix} next={next}", prefix + 1),
            stats,
        );

        let (mut runtime, mut consumer) = make_runtime(judge(&notes), &[1], vec![], 4);
        for id in 1..=prefix {
            let at = id as i64 * 100;
            let down = button(55, at, id as u64 * 2 - 1, ButtonState::Down);
            let result = runtime
                .process_input(down.clone(), &NoMapping, point(2, 9000))
                .unwrap();
            report(&result, &down, &[1], &[hit(id as u64, at, &down)], at);
            no_audio(&result);
            let up = button(55, at, id as u64 * 2, ButtonState::Up);
            let result = runtime
                .process_input(up.clone(), &NoMapping, point(2, 9000))
                .unwrap();
            report(&result, &up, &[1], &[], at);
            no_audio(&result);
            assert_eq!(
                runtime.judge().state(ObjectId(id as u64)),
                Some(InteractionState::Completed)
            );
        }
        assert_eq!(runtime.telemetry().counters().judge_results, prefix as u64);
        assert_eq!(runtime.telemetry().counters().inputs, prefix as u64 * 2);
        assert_eq!(
            runtime.judge().state(ObjectId(next)),
            Some(InteractionState::Pending)
        );
        assert!(!runtime.judge().is_held(owner(55, 1)));
        let input = button(55, at, next * 2 - 1, ButtonState::Down);
        let consumed = input.clone();
        let (result, stats) = measure(|| {
            runtime.process_input_report(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
        });
        let result = result.unwrap();
        report(&result.report, &input, &[1], &[hit(next, at, &input)], at);
        disposition(
            result.dispositions[0],
            InputFreshness::FreshPress,
            Some(next),
            1,
        );
        assert_eq!(result.dispositions.len(), 1);
        assert_eq!(result.refused, None);
        no_audio(&result.report);
        assert_eq!(
            runtime.judge().state(ObjectId(next)),
            Some(InteractionState::Completed)
        );
        assert_eq!(runtime.telemetry().counters().judge_results, next);
        assert_eq!(runtime.telemetry().counters().inputs, prefix as u64 * 2 + 1);
        assert_eq!(runtime.telemetry().processing(), None);
        queue_empty(&mut consumer);
        raw(
            match prefix {
                0 => "runtime.prefix.0",
                1 => "runtime.association.reused",
                _ => "runtime.prefix.4096",
            },
            &format!(
                "chart={} completed_prefix={prefix} clock=Disabled",
                prefix + 1
            ),
            stats,
        );
    }
}

#[test]
fn runtime_legacy_observed_fanout_and_freshness_costs_preserve_original_inputs() {
    for observed in [false, true] {
        let (mut runtime, mut consumer) =
            make_runtime(judge(&[(17, 100, 1)]), &[1], vec![sound(17, 17)], 4);
        let input = button(55, 100, 91, ButtonState::Down);
        let consumed = input.clone();
        if observed {
            let (result, stats) = measure(|| {
                runtime.process_input_report(
                    black_box(consumed),
                    &NoMapping,
                    black_box(point(2, 9000)),
                )
            });
            let result = result.unwrap();
            report(&result.report, &input, &[1], &[hit(17, 100, &input)], 100);
            assert_eq!(result.dispositions.len(), 1);
            disposition(
                result.dispositions[0],
                InputFreshness::FreshPress,
                Some(17),
                1,
            );
            assert_eq!(result.refused, None);
            assert_eq!(result.report.audio_commands, [command(17)]);
            assert!(result.report.audio_failures.is_empty());
            raw(
                "runtime.observed.hit.fresh",
                "bindings=1 sounds=1 clock=Disabled",
                stats,
            );
        } else {
            let (result, stats) = measure(|| {
                runtime.process_input(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
            });
            let result = result.unwrap();
            report(&result, &input, &[1], &[hit(17, 100, &input)], 100);
            assert_eq!(result.audio_commands, [command(17)]);
            assert!(result.audio_failures.is_empty());
            raw(
                "runtime.legacy.hit.fresh",
                "bindings=1 sounds=1 clock=Disabled",
                stats,
            );
        }
        assert_eq!(
            runtime.judge().state(ObjectId(17)),
            Some(InteractionState::Completed)
        );
        assert!(runtime.judge().is_held(owner(55, 1)));
        assert_eq!(runtime.telemetry().counters().inputs, 1);
        assert_eq!(runtime.telemetry().counters().judge_results, 1);
        assert_eq!(runtime.telemetry().counters().audio_commands, 1);
        assert_eq!(runtime.telemetry().processing(), None);
        assert_eq!(consumer.try_pop().unwrap(), command(17));
        queue_empty(&mut consumer);
    }
    let (mut runtime, mut consumer) = make_runtime(
        judge(&[(1, 100, 1), (2, 100, 2)]),
        &[2, 1],
        vec![sound(1, 1), sound(2, 2)],
        4,
    );
    let input = button(55, 100, 91, ButtonState::Down);
    let consumed = input.clone();
    let (result, stats) = measure(|| {
        runtime.process_input_report(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
    });
    let result = result.unwrap();
    report(
        &result.report,
        &input,
        &[2, 1],
        &[hit(2, 100, &input), hit(1, 100, &input)],
        100,
    );
    assert_eq!(result.dispositions.len(), 2);
    disposition(
        result.dispositions[0],
        InputFreshness::FreshPress,
        Some(2),
        1,
    );
    disposition(
        result.dispositions[1],
        InputFreshness::FreshPress,
        Some(1),
        1,
    );
    assert_eq!(result.refused, None);
    assert_eq!(result.report.audio_commands, [command(2), command(1)]);
    assert!(result.report.audio_failures.is_empty());
    for id in [1, 2] {
        assert_eq!(
            runtime.judge().state(ObjectId(id)),
            Some(InteractionState::Completed)
        );
        assert!(runtime.judge().is_held(owner(55, id as u32)));
    }
    assert_eq!(consumer.try_pop().unwrap(), command(2));
    assert_eq!(consumer.try_pop().unwrap(), command(1));
    queue_empty(&mut consumer);
    assert_eq!(runtime.telemetry().counters().judge_results, 2);
    raw(
        "runtime.observed.fanout",
        "binding_order=2,1 destinations=2 clock=Disabled",
        stats,
    );

    for (name, state, unbound, freshness, held) in [
        (
            "runtime.observed.unbound",
            ButtonState::Down,
            true,
            InputFreshness::FreshPress,
            false,
        ),
        (
            "runtime.observed.held",
            ButtonState::Down,
            false,
            InputFreshness::HeldDown,
            true,
        ),
        (
            "runtime.observed.repeat",
            ButtonState::Repeat,
            false,
            InputFreshness::ExplicitRepeat,
            true,
        ),
        (
            "runtime.observed.up",
            ButtonState::Up,
            false,
            InputFreshness::Other,
            false,
        ),
    ] {
        let (mut runtime, mut consumer) = make_runtime(judge(&[(17, 100, 1)]), &[1], vec![], 4);
        if !unbound {
            let warm = button(55, 100, 90, ButtonState::Down);
            let r = runtime
                .process_input(warm.clone(), &NoMapping, point(2, 9000))
                .unwrap();
            report(&r, &warm, &[1], &[hit(17, 100, &warm)], 100);
        }
        let mut input = button(55, 100, 91, state);
        if unbound {
            if let PhysicalInputEvent::Button(b) = &mut input {
                b.control = PhysicalControlId::keyboard(5);
            }
        }
        let consumed = input.clone();
        let (result, stats) = measure(|| {
            runtime.process_input_report(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
        });
        let result = result.unwrap();
        report(
            &result.report,
            &input,
            if unbound { &[] } else { &[1] },
            &[],
            100,
        );
        no_audio(&result.report);
        assert_eq!(result.refused, None);
        if unbound {
            assert!(result.dispositions.is_empty());
            assert_eq!(runtime.telemetry().counters().unbound, 1);
            assert_eq!(
                runtime.judge().state(ObjectId(17)),
                Some(InteractionState::Pending)
            );
            assert_eq!(runtime.judge().effective_song_time(), None);
        } else {
            assert_eq!(result.dispositions.len(), 1);
            disposition(result.dispositions[0], freshness, None, 0);
            assert_eq!(runtime.telemetry().counters().unbound, 0);
            assert_eq!(
                runtime.judge().state(ObjectId(17)),
                Some(InteractionState::Completed)
            );
        }
        assert_eq!(runtime.judge().is_held(owner(55, 1)), held);
        queue_empty(&mut consumer);
        raw(name, "clock=Disabled independent_fixture=true", stats);
    }
}

#[test]
fn runtime_preflight_refusals_do_not_commit_judge_sequence_or_audio() {
    for case in ["input-domain", "audio-domain", "sequence", "host-time"] {
        let (mut runtime, mut consumer) =
            make_runtime(judge(&[(2, 200, 1)]), &[1], vec![sound(2, 2)], 4);
        // A genuine admitted operation establishes source sequence10 and host1100.
        let warm = button(55, 100, 10, ButtonState::Up);
        let first = runtime
            .process_input(warm.clone(), &NoMapping, point(2, 9000))
            .unwrap();
        report(&first, &warm, &[1], &[], 100);
        runtime.enqueue_audio(sentinel()).unwrap();
        let before_hash = runtime.judge().stable_hash().unwrap();
        let before_counters = runtime.telemetry().counters();
        let mut input = button(55, 200, 11, ButtonState::Down);
        let mut audio = point(2, 9000);
        let expected = match case {
            "input-domain" => {
                input.meta_mut().clock_domain = ClockDomainId(71);
                RuntimeError::UnmappedClock {
                    from: ClockDomainId(71),
                    to: ClockDomainId(1),
                }
            }
            "audio-domain" => {
                audio.domain = ClockDomainId(72);
                RuntimeError::UnmappedClock {
                    from: ClockDomainId(72),
                    to: ClockDomainId(2),
                }
            }
            "sequence" => {
                input.meta_mut().sequence = 9; // Strictly smaller, not legal equal fanout.
                RuntimeError::SequenceRegression {
                    device: DeviceId(55),
                    last: 10,
                    received: 9,
                }
            }
            "host-time" => {
                input.meta_mut().timestamp = ts(1099);
                RuntimeError::NonMonotonicHost
            }
            _ => unreachable!(),
        };
        let (result, stats) = measure(|| {
            runtime.process_input_report(black_box(input), &NoMapping, black_box(audio))
        });
        assert_eq!(result.unwrap_err(), expected);
        assert_eq!(runtime.judge().stable_hash().unwrap(), before_hash);
        assert_eq!(
            runtime.judge().state(ObjectId(2)),
            Some(InteractionState::Pending)
        );
        assert!(!runtime.judge().is_held(owner(55, 1)));
        let after = runtime.telemetry().counters();
        assert_eq!(after.inputs, before_counters.inputs);
        assert_eq!(after.judge_results, before_counters.judge_results);
        assert_eq!(after.audio_commands, before_counters.audio_commands);
        assert_eq!(after.rejected, before_counters.rejected + 1);
        assert_eq!(consumer.try_pop().unwrap(), sentinel());
        queue_empty(&mut consumer);
        // Corrected input with equal prior sequence10 is legal: refusal did not
        // consume seq11 or host1200. This retry is deliberately unmeasured.
        let corrected = button(55, 200, 10, ButtonState::Down);
        let r = runtime
            .process_input(corrected.clone(), &NoMapping, point(2, 9000))
            .unwrap();
        report(&r, &corrected, &[1], &[hit(2, 200, &corrected)], 200);
        assert_eq!(r.audio_commands, [command(2)]);
        assert!(r.audio_failures.is_empty());
        assert_eq!(consumer.try_pop().unwrap(), command(2));
        queue_empty(&mut consumer);
        raw(
            &format!("runtime.refused.{case}"),
            "independent_fixture seq10->9 clock=Disabled",
            stats,
        );
    }
}

#[test]
fn runtime_audio_admitted_full_and_disconnected_preserve_exact_commands_and_commits() {
    for case in ["admitted", "full", "disconnected"] {
        let (mut runtime, consumer) =
            make_runtime(judge(&[(17, 100, 1)]), &[1], vec![sound(17, 17)], 1);
        let mut consumer = Some(consumer);
        if case != "disconnected" {
            runtime.enqueue_audio(sentinel()).unwrap();
        }
        if case == "admitted" {
            assert_eq!(consumer.as_mut().unwrap().try_pop().unwrap(), sentinel());
        }
        if case == "disconnected" {
            drop(consumer.take());
        }
        // Explicit enqueue_audio uses the same admission telemetry as judged
        // keysounds. Popping the setup sentinel changes FIFO occupancy, not
        // this cumulative counter, so preserve the genuine setup baseline.
        let before_counters = runtime.telemetry().counters();
        assert_eq!(
            before_counters.audio_commands,
            u64::from(case != "disconnected")
        );
        assert_eq!(before_counters.inputs, 0);
        assert_eq!(before_counters.judge_results, 0);
        assert_eq!(before_counters.queue_full, 0);
        assert_eq!(before_counters.queue_disconnected, 0);
        let input = button(55, 100, 91, ButtonState::Down);
        let consumed = input.clone();
        let (result, stats) = measure(|| {
            runtime.process_input_report(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
        });
        let result = result.unwrap();
        report(&result.report, &input, &[1], &[hit(17, 100, &input)], 100);
        assert_eq!(result.dispositions.len(), 1);
        disposition(
            result.dispositions[0],
            InputFreshness::FreshPress,
            Some(17),
            1,
        );
        assert_eq!(result.refused, None);
        assert_eq!(
            runtime.judge().state(ObjectId(17)),
            Some(InteractionState::Completed)
        );
        assert!(runtime.judge().is_held(owner(55, 1)));
        assert_eq!(
            runtime.telemetry().counters().judge_results,
            before_counters.judge_results + 1
        );
        if case == "admitted" {
            assert_eq!(result.report.audio_commands, [command(17)]);
            assert!(result.report.audio_failures.is_empty());
            assert_eq!(consumer.as_mut().unwrap().try_pop().unwrap(), command(17));
            queue_empty(consumer.as_mut().unwrap());
        } else {
            assert!(result.report.audio_commands.is_empty());
            assert_eq!(result.report.audio_failures.len(), 1);
            assert_eq!(result.report.audio_failures[0].command, command(17));
            assert_eq!(
                result.report.audio_failures[0].reason,
                if case == "full" {
                    QueuePushError::Full
                } else {
                    QueuePushError::Disconnected
                }
            );
            if case == "full" {
                assert_eq!(consumer.as_mut().unwrap().try_pop().unwrap(), sentinel());
                queue_empty(consumer.as_mut().unwrap());
                assert_eq!(
                    runtime.telemetry().counters().queue_full,
                    before_counters.queue_full + 1
                );
            } else {
                assert_eq!(
                    runtime.telemetry().counters().queue_disconnected,
                    before_counters.queue_disconnected + 1
                );
            }
        }
        assert_eq!(
            runtime.telemetry().counters().audio_commands,
            before_counters.audio_commands + u64::from(case == "admitted")
        );
        let mut expected_counters = before_counters;
        expected_counters.inputs += 1;
        expected_counters.judge_results += 1;
        expected_counters.audio_commands += u64::from(case == "admitted");
        expected_counters.queue_full += u64::from(case == "full");
        expected_counters.queue_disconnected += u64::from(case == "disconnected");
        assert_eq!(runtime.telemetry().counters(), expected_counters);
        raw(
            &format!("runtime.audio.{case}"),
            "capacity=1 original_fifo clock=Disabled",
            stats,
        );
    }
}

fn member(player: u32, automatic: bool) -> MemberConfig {
    let device = (!automatic).then_some(DeviceId(1000 + u64::from(player)));
    MemberConfig {
        player: PlayerId(player),
        device,
        bindings: bindings(
            &[1],
            device.map_or(DeviceSelector::Any, DeviceSelector::Exact),
        ),
        judge: judge(&[(17, 100, 1)]),
        sounds: vec![sound(17, u64::from(player))],
    }
}
fn make_group(count: usize, automatic: bool, capacity: usize) -> (RuntimeGroup, CommandConsumer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut group = RuntimeGroup::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(1000), ts(0), Rate::NORMAL),
        producer,
        (1..=count).map(|p| member(p as u32, automatic)).collect(),
        8,
        &[],
    )
    .unwrap();
    group.set_processing_clock(RuntimeProcessingClock::Disabled);
    (group, consumer)
}
fn make_solo(capacity: usize) -> (SoloRuntime, CommandConsumer) {
    let (producer, consumer) = command_queue(capacity).unwrap();
    let mut solo = SoloRuntime::new(
        ClockDomainId(1),
        ClockDomainId(2),
        Transport::new(ts(1000), ts(0), Rate::NORMAL),
        bindings(&[1], DeviceSelector::Any),
        judge(&[(17, 100, 1)]),
        producer,
        vec![sound(17, 1)],
        8,
    )
    .unwrap();
    solo.set_processing_clock(RuntimeProcessingClock::Disabled);
    (solo, consumer)
}

#[test]
fn local_groups_route_first_last_and_unassigned_to_one_owner_through_64_members() {
    for count in [1_usize, 2, 4, 64] {
        for route in ["first", "last", "unassigned"] {
            let (mut group, mut consumer) = make_group(count, false, 4);
            let before: Vec<_> = (1..=count)
                .map(|p| {
                    group
                        .member_judge(PlayerId(p as u32))
                        .unwrap()
                        .stable_hash()
                        .unwrap()
                })
                .collect();
            let player = if route == "first" { 1 } else { count as u32 };
            let source = if route == "unassigned" {
                9_999_999
            } else {
                1000 + u64::from(player)
            };
            let input = button(source, 100, 91, ButtonState::Down);
            let consumed = input.clone();
            let (result, stats) = measure(|| {
                group.process_input(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
            });
            match result.unwrap() {
                InputResult::Ignored { device } => {
                    assert_eq!(route, "unassigned");
                    assert_eq!(device, DeviceId(source));
                    queue_empty(&mut consumer);
                }
                InputResult::Processed(reports) => {
                    assert_ne!(route, "unassigned");
                    assert_eq!(reports.len(), 1);
                    assert_eq!(reports[0].player, PlayerId(player));
                    report(
                        &reports[0].report,
                        &input,
                        &[1],
                        &[hit(17, 100, &input)],
                        100,
                    );
                    assert_eq!(
                        reports[0].report.audio_commands,
                        [command(u64::from(player))]
                    );
                    assert!(reports[0].report.audio_failures.is_empty());
                    assert_eq!(consumer.try_pop().unwrap(), command(u64::from(player)));
                    queue_empty(&mut consumer);
                }
            }
            assert!(!group.poisoned());
            for p in 1..=count {
                let acted = route != "unassigned" && p as u32 == player;
                let judge = group.member_judge(PlayerId(p as u32)).unwrap();
                assert_eq!(
                    judge.state(ObjectId(17)),
                    Some(if acted {
                        InteractionState::Completed
                    } else {
                        InteractionState::Pending
                    })
                );
                assert_eq!(judge.is_held(owner(source, 1)), acted);
                assert_eq!(
                    judge.effective_song_time(),
                    if acted { Some(ts(100)) } else { None }
                );
                if !acted {
                    assert_eq!(judge.stable_hash().unwrap(), before[p - 1]);
                }
                let counters = group
                    .member_telemetry(PlayerId(p as u32))
                    .unwrap()
                    .counters();
                assert_eq!(
                    (
                        counters.inputs,
                        counters.judge_results,
                        counters.audio_commands
                    ),
                    if acted { (1, 1, 1) } else { (0, 0, 0) }
                );
                assert_eq!(
                    group
                        .member_telemetry(PlayerId(p as u32))
                        .unwrap()
                        .processing(),
                    None
                );
            }
            raw(
                &format!("group.{count}.{route}"),
                "exact_distinct_device_ids unique_players one_owner clock=Disabled",
                stats,
            );
        }
    }
    let (mut group, mut consumer) = make_group(1, true, 4);
    let input = button(9_876_543_210, 100, 91, ButtonState::Down);
    let consumed = input.clone();
    let (result, stats) =
        measure(|| group.process_input(black_box(consumed), &NoMapping, black_box(point(2, 9000))));
    let InputResult::Processed(reports) = result.unwrap() else {
        panic!("automatic owner must accept arbitrary source");
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].player, PlayerId(1));
    report(
        &reports[0].report,
        &input,
        &[1],
        &[hit(17, 100, &input)],
        100,
    );
    assert_eq!(reports[0].report.audio_commands, [command(1)]);
    assert!(reports[0].report.audio_failures.is_empty());
    assert_eq!(
        group.member_judge(PlayerId(1)).unwrap().state(ObjectId(17)),
        Some(InteractionState::Completed)
    );
    assert!(group
        .member_judge(PlayerId(1))
        .unwrap()
        .is_held(owner(9_876_543_210, 1)));
    assert_eq!(consumer.try_pop().unwrap(), command(1));
    queue_empty(&mut consumer);
    assert!(!group.poisoned());
    raw(
        "group.1.automatic",
        "arbitrary_source=9876543210 clock=Disabled",
        stats,
    );
}

#[test]
fn solo_partial_reports_and_group_reported_failure_have_distinct_poison_semantics() {
    for case in ["admitted", "full", "disconnected"] {
        let (mut solo, consumer) = make_solo(1);
        let mut consumer = Some(consumer);
        if case == "full" {
            solo.enqueue_audio(sentinel()).unwrap();
        }
        if case == "disconnected" {
            drop(consumer.take());
        }
        let input = button(8_765_432_109, 100, 91, ButtonState::Down);
        let consumed = input.clone();
        let (result, stats) = measure(|| {
            solo.process_input(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
        });
        // This adapter intentionally returns Ok(original partial report), unlike
        // RuntimeGroup's Err(ReportedFailure). Do not unwrap_err on Solo here.
        let result = result.unwrap();
        report(&result, &input, &[1], &[hit(17, 100, &input)], 100);
        assert_eq!(
            solo.judge().state(ObjectId(17)),
            Some(InteractionState::Completed)
        );
        assert!(solo.judge().is_held(owner(8_765_432_109, 1)));
        if case == "admitted" {
            assert_eq!(result.audio_commands, [command(1)]);
            assert!(result.audio_failures.is_empty());
            assert_eq!(consumer.as_mut().unwrap().try_pop().unwrap(), command(1));
            queue_empty(consumer.as_mut().unwrap());
        } else {
            assert!(result.audio_commands.is_empty());
            assert_eq!(result.audio_failures.len(), 1);
            assert_eq!(result.audio_failures[0].command, command(1));
            assert_eq!(
                result.audio_failures[0].reason,
                if case == "full" {
                    QueuePushError::Full
                } else {
                    QueuePushError::Disconnected
                }
            );
            let before = solo.judge().stable_hash().unwrap();
            let next = button(8_765_432_109, 100, 92, ButtonState::Up);
            let (poison, counts) = measure(|| {
                solo.process_input(black_box(next), &NoMapping, black_box(point(2, 9000)))
            });
            let poison = poison.unwrap_err();
            assert!(matches!(poison.kind, FailureKind::Poisoned));
            assert_eq!(poison.failed_player, None);
            assert!(poison.completed_reports.is_empty());
            assert_eq!(solo.judge().stable_hash().unwrap(), before);
            if case == "full" {
                assert_eq!(consumer.as_mut().unwrap().try_pop().unwrap(), sentinel());
                queue_empty(consumer.as_mut().unwrap());
            }
            raw(
                &format!("solo.poisoned.{case}"),
                "separate_followup_no_commit",
                counts,
            );
        }
        assert_eq!(solo.telemetry().counters().judge_results, 1);
        raw(
            &format!("solo.audio.{case}"),
            "automatic arbitrary_source=8765432109 capacity=1 clock=Disabled",
            stats,
        );
    }
    for case in ["full", "disconnected", "core-domain"] {
        let (mut group, consumer) = make_group(2, false, 1);
        let mut consumer = Some(consumer);
        if case == "full" {
            group.enqueue_audio(sentinel()).unwrap();
        }
        if case == "disconnected" {
            drop(consumer.take());
        }
        let input = button(1002, 100, 91, ButtonState::Down);
        let consumed = input.clone();
        let audio = if case == "core-domain" {
            point(72, 9000)
        } else {
            point(2, 9000)
        };
        let before = group
            .member_judge(PlayerId(2))
            .unwrap()
            .stable_hash()
            .unwrap();
        let (result, stats) =
            measure(|| group.process_input(black_box(consumed), &NoMapping, black_box(audio)));
        let error = result.unwrap_err();
        assert_eq!(error.failed_player, Some(PlayerId(2)));
        if case == "core-domain" {
            assert!(matches!(
                error.kind,
                FailureKind::Core(RuntimeError::UnmappedClock {
                    from: ClockDomainId(72),
                    to: ClockDomainId(2)
                })
            ));
            assert!(error.completed_reports.is_empty());
            assert_eq!(
                group
                    .member_judge(PlayerId(2))
                    .unwrap()
                    .stable_hash()
                    .unwrap(),
                before
            );
            queue_empty(consumer.as_mut().unwrap());
        } else {
            assert!(matches!(error.kind, FailureKind::ReportedFailure));
            assert_eq!(error.completed_reports.len(), 1);
            assert_eq!(error.completed_reports[0].player, PlayerId(2));
            let r = &error.completed_reports[0].report;
            report(r, &input, &[1], &[hit(17, 100, &input)], 100);
            assert!(r.audio_commands.is_empty());
            assert_eq!(r.audio_failures.len(), 1);
            assert_eq!(r.audio_failures[0].command, command(2));
            assert_eq!(
                r.audio_failures[0].reason,
                if case == "full" {
                    QueuePushError::Full
                } else {
                    QueuePushError::Disconnected
                }
            );
            assert_eq!(
                group.member_judge(PlayerId(2)).unwrap().state(ObjectId(17)),
                Some(InteractionState::Completed)
            );
            assert_eq!(
                group
                    .member_telemetry(PlayerId(2))
                    .unwrap()
                    .counters()
                    .judge_results,
                1
            );
            if case == "full" {
                assert_eq!(consumer.as_mut().unwrap().try_pop().unwrap(), sentinel());
                queue_empty(consumer.as_mut().unwrap());
            }
        }
        assert!(group.poisoned());
        assert_eq!(
            group.member_judge(PlayerId(1)).unwrap().state(ObjectId(17)),
            Some(InteractionState::Pending)
        );
        assert_eq!(
            group
                .member_telemetry(PlayerId(1))
                .unwrap()
                .counters()
                .inputs,
            0
        );
        let hash = group
            .member_judge(PlayerId(2))
            .unwrap()
            .stable_hash()
            .unwrap();
        let next = button(1001, 100, 92, ButtonState::Down);
        let (poison, counts) =
            measure(|| group.process_input(black_box(next), &NoMapping, black_box(point(2, 9000))));
        let poison = poison.unwrap_err();
        assert!(matches!(poison.kind, FailureKind::Poisoned));
        assert_eq!(poison.failed_player, None);
        assert!(poison.completed_reports.is_empty());
        assert_eq!(
            group
                .member_judge(PlayerId(2))
                .unwrap()
                .stable_hash()
                .unwrap(),
            hash
        );
        assert_eq!(
            group.member_judge(PlayerId(1)).unwrap().state(ObjectId(17)),
            Some(InteractionState::Pending)
        );
        raw(
            &format!("group.poisoned.{case}"),
            "followup_other_owner_no_commit",
            counts,
        );
        raw(
            &format!("group.failure.{case}"),
            "players=2 last_owner capacity=1 clock=Disabled",
            stats,
        );
    }
}

#[test]
fn raw_hid_returned_payload_and_consumed_unassigned_payload_have_distinct_drop_boundaries() {
    let (mut runtime, mut consumer) = make_runtime(judge(&[(17, 100, 1)]), &[1], vec![], 4);
    let bytes: Vec<_> = (0..256).map(|n| (n as u8).wrapping_mul(3)).collect();
    let expected = bytes.clone();
    let pointer = bytes.as_ptr();
    let meta = *button(55, 100, 91, ButtonState::Down).meta();
    let input = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
        meta,
        report_id: Some(7),
        data: bytes,
    });
    let before = runtime.judge().stable_hash().unwrap();
    let (result, stats) = measure(|| {
        runtime.process_input_report(black_box(input), &NoMapping, black_box(point(2, 9000)))
    });
    let result = result.unwrap();
    let Some(PhysicalInputEvent::RawHidReport(raw_input)) = result.report.input.as_ref() else {
        panic!("original raw payload must remain owned by report");
    };
    assert_eq!(raw_input.meta, meta);
    assert_eq!(raw_input.report_id, Some(7));
    assert_eq!(raw_input.data, expected);
    assert_eq!(raw_input.data.as_ptr(), pointer);
    assert!(result.report.bound_inputs.is_empty());
    assert!(result.report.judge_events.is_empty());
    assert!(result.report.hazard_events.is_empty());
    assert_eq!(result.report.judge_error, None);
    assert_eq!(result.report.song_time, ts(100));
    assert_eq!(result.report.audio_at, point(2, 9000));
    assert!(result.dispositions.is_empty());
    assert_eq!(result.refused, None);
    no_audio(&result.report);
    assert_eq!(runtime.telemetry().counters().inputs, 1);
    assert_eq!(runtime.telemetry().counters().unbound, 1);
    assert_eq!(runtime.judge().stable_hash().unwrap(), before);
    assert_eq!(
        stats.dealloc,
        Operation::default(),
        "returned payload must not be dropped inside call"
    );
    queue_empty(&mut consumer);
    raw(
        "runtime.raw-hid.returned",
        "payload=256 report_retains_original_storage clock=Disabled",
        stats,
    );
    drop(result); // Outside the runtime measurement, including owned data256.

    let (mut group, mut consumer) = make_group(64, false, 4);
    let before: Vec<_> = (1..=64)
        .map(|p| {
            group
                .member_judge(PlayerId(p))
                .unwrap()
                .stable_hash()
                .unwrap()
        })
        .collect();
    let payload: Vec<_> = (0..257).map(|n| (n as u8).wrapping_add(11)).collect();
    assert_eq!(payload.len(), 257);
    assert_eq!(payload.capacity(), 257);
    let input = PhysicalInputEvent::RawHidReport(RawHidReportEvent {
        meta: *button(9_999_999, 100, 91, ButtonState::Down).meta(),
        report_id: Some(9),
        data: payload,
    });
    let (result, stats) =
        measure(|| group.process_input(black_box(input), &NoMapping, black_box(point(2, 9000))));
    assert!(matches!(
        result.unwrap(),
        InputResult::Ignored {
            device: DeviceId(9_999_999)
        }
    ));
    // Owned input is consumed before Ignored returns: its known payload layout
    // is a real deallocation inside the call, not a returned-report disposal.
    assert_eq!(stats.dealloc, one(257));
    assert!(!group.poisoned());
    for p in 1..=64 {
        assert_eq!(
            group
                .member_judge(PlayerId(p))
                .unwrap()
                .stable_hash()
                .unwrap(),
            before[p as usize - 1]
        );
        assert_eq!(
            group
                .member_telemetry(PlayerId(p))
                .unwrap()
                .counters()
                .inputs,
            0
        );
    }
    queue_empty(&mut consumer);
    raw(
        "group.64.raw-hid.consumed",
        "unassigned payload=257 consumed_input_drop_inside",
        stats,
    );
}

#[derive(Clone, Copy)]
struct ClockScript {
    readings: [Option<u64>; 2],
    calls: usize,
}
thread_local! {
    static CLOCK_SCRIPT: Cell<ClockScript> = const { Cell::new(ClockScript { readings: [None; 2], calls: 0 }) };
}
fn script(readings: [Option<u64>; 2]) {
    CLOCK_SCRIPT.with(|slot| slot.set(ClockScript { readings, calls: 0 }));
}
fn scripted_clock() -> Option<u64> {
    CLOCK_SCRIPT.with(|slot| {
        let mut state = slot.get();
        let value = state.readings.get(state.calls).copied().flatten();
        state.calls = state.calls.saturating_add(1);
        slot.set(state);
        value
    })
}

#[test]
fn external_processing_clock_reads_twice_on_acceptance_and_preflight_refusal() {
    for rejected in [false, true] {
        let (mut runtime, mut consumer) =
            make_runtime(judge(&[(17, 100, 1)]), &[1], vec![sound(17, 17)], 4);
        runtime.set_processing_clock(RuntimeProcessingClock::External(scripted_clock));
        script(if rejected {
            [Some(100), Some(117)]
        } else {
            [Some(96), Some(109)]
        });
        let mut input = button(55, 100, 91, ButtonState::Down);
        if rejected {
            input.meta_mut().clock_domain = ClockDomainId(71);
        }
        let consumed = input.clone();
        let hash = runtime.judge().stable_hash().unwrap();
        let (result, stats) = measure(|| {
            runtime.process_input_report(black_box(consumed), &NoMapping, black_box(point(2, 9000)))
        });
        assert_eq!(CLOCK_SCRIPT.with(|slot| slot.get().calls), 2);
        let timing = runtime.telemetry().processing().unwrap();
        let delta = if rejected { 17 } else { 13 };
        assert_eq!(
            (
                timing.samples,
                timing.p50_ns,
                timing.p95_ns,
                timing.p99_ns,
                timing.max_ns
            ),
            (1, delta, delta, delta, delta)
        );
        if rejected {
            assert_eq!(
                result.unwrap_err(),
                RuntimeError::UnmappedClock {
                    from: ClockDomainId(71),
                    to: ClockDomainId(1)
                }
            );
            assert_eq!(runtime.judge().stable_hash().unwrap(), hash);
            assert_eq!(runtime.telemetry().counters().inputs, 0);
            assert_eq!(runtime.telemetry().counters().rejected, 1);
            queue_empty(&mut consumer);
        } else {
            let result = result.unwrap();
            report(&result.report, &input, &[1], &[hit(17, 100, &input)], 100);
            assert_eq!(result.dispositions.len(), 1);
            disposition(
                result.dispositions[0],
                InputFreshness::FreshPress,
                Some(17),
                1,
            );
            assert_eq!(result.refused, None);
            assert_eq!(result.report.audio_commands, [command(17)]);
            assert!(result.report.audio_failures.is_empty());
            assert_eq!(runtime.telemetry().counters().inputs, 1);
            assert_eq!(runtime.telemetry().counters().rejected, 0);
            assert_eq!(consumer.try_pop().unwrap(), command(17));
            queue_empty(&mut consumer);
        }
        raw(
            if rejected {
                "runtime.external.refused"
            } else {
                "runtime.external.accepted"
            },
            "clock=External scripted2reads original_game_time=100 audio=9000",
            stats,
        );
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MapperCalls {
    calls: [Option<(ClockPoint, ClockDomainId)>; 2],
    maps: usize,
    qualities: usize,
    overflow: bool,
}
struct AllocatingMapper {
    calls: Cell<MapperCalls>,
}
impl ClockMapper for AllocatingMapper {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        let mut calls = self.calls.get();
        if calls.maps < calls.calls.len() {
            calls.calls[calls.maps] = Some((from, to));
        } else {
            calls.overflow = true;
        }
        calls.maps = calls.maps.saturating_add(1);
        self.calls.set(calls);
        // A deliberately allocating injected callback proves that its owned work
        // is included in the public boundary. Data/content and destruction are
        // opaque to the optimizer; no assertions/logging occur in this callback.
        let payload = vec![black_box(0x69_u8); black_box(31)];
        black_box(&payload);
        let result = match (from.domain.0, to.0) {
            (9, 1) => from.timestamp.checked_add(Duration::from_nanos(1000)),
            (8, 2) => from.timestamp.checked_add(Duration::from_nanos(8000)),
            _ => None,
        };
        drop(black_box(payload));
        result
    }
    fn quality(&self) -> ClockMappingQuality {
        let mut calls = self.calls.get();
        calls.qualities = calls.qualities.saturating_add(1);
        self.calls.set(calls);
        ClockMappingQuality::Estimated {
            max_error: Duration::from_nanos(2),
        }
    }
}

#[test]
fn injected_allocating_mapper_preserves_original_clock_provenance_and_counts_its_work() {
    let mapper = AllocatingMapper {
        calls: Cell::new(MapperCalls::default()),
    };
    let (mut runtime, mut consumer) =
        make_runtime(judge(&[(17, 100, 1)]), &[1], vec![sound(17, 17)], 4);
    let mut input = button(55, 100, 91, ButtonState::Down);
    input.meta_mut().clock_domain = ClockDomainId(9);
    input.meta_mut().timestamp = ts(100);
    let original_meta = *input.meta();
    let consumed = input.clone();
    let (result, stats) = measure(|| {
        runtime.process_input_report(
            black_box(consumed),
            black_box(&mapper as &dyn ClockMapper),
            black_box(point(8, 1000)),
        )
    });
    let result = result.unwrap();
    assert_eq!(
        mapper.calls.get(),
        MapperCalls {
            calls: [
                Some((point(9, 100), ClockDomainId(1))),
                Some((point(8, 1000), ClockDomainId(2)))
            ],
            maps: 2,
            qualities: 2,
            overflow: false,
        }
    );
    let mut normalized = input;
    normalized.meta_mut().clock_domain = ClockDomainId(1);
    normalized.meta_mut().timestamp = ts(1100);
    normalized.meta_mut().original_clock_point = Some(point(9, 100));
    assert_eq!(normalized.meta().native, original_meta.native);
    assert_eq!(normalized.meta().sequence, 91);
    assert_eq!(normalized.meta().source, DeviceId(55));
    assert_eq!(result.report.input.as_ref(), Some(&normalized));
    assert_eq!(result.report.bound_inputs, [bound(normalized.clone(), 1)]);
    assert_eq!(result.report.judge_events, [hit(17, 100, &normalized)]);
    assert_eq!(result.report.song_time, ts(100));
    assert_eq!(result.report.audio_at, point(2, 9000));
    assert_eq!(
        result.report.input_mapping_quality,
        ClockMappingQuality::Estimated {
            max_error: Duration::from_nanos(2)
        }
    );
    assert_eq!(
        result.report.audio_mapping_quality,
        ClockMappingQuality::Estimated {
            max_error: Duration::from_nanos(2)
        }
    );
    assert_eq!(result.report.judge_error, None);
    assert!(result.report.hazard_events.is_empty());
    assert_eq!(result.report.audio_commands, [command(17)]);
    assert!(result.report.audio_failures.is_empty());
    assert_eq!(result.dispositions.len(), 1);
    disposition(
        result.dispositions[0],
        InputFreshness::FreshPress,
        Some(17),
        1,
    );
    assert_eq!(result.refused, None);
    assert_eq!(
        runtime.judge().state(ObjectId(17)),
        Some(InteractionState::Completed)
    );
    assert!(runtime.judge().is_held(owner(55, 1)));
    assert_eq!(consumer.try_pop().unwrap(), command(17));
    queue_empty(&mut consumer);
    assert_eq!(runtime.telemetry().processing(), None);
    // An independent same-domain fixture executes the same real business
    // transition with the already-normalized metadata. Only the mapper's two
    // explicit allocations/drops are absent; compare complete operation units.
    let (mut control, mut control_consumer) =
        make_runtime(judge(&[(17, 100, 1)]), &[1], vec![sound(17, 17)], 4);
    let control_input = normalized.clone();
    let (control_result, control_stats) = measure(|| {
        control.process_input_report(
            black_box(control_input),
            &NoMapping,
            black_box(point(2, 9000)),
        )
    });
    let control_result = control_result.unwrap();
    report(
        &control_result.report,
        &normalized,
        &[1],
        &[hit(17, 100, &normalized)],
        100,
    );
    assert_eq!(control_result.report.audio_commands, [command(17)]);
    assert!(control_result.report.audio_failures.is_empty());
    disposition(
        control_result.dispositions[0],
        InputFreshness::FreshPress,
        Some(17),
        1,
    );
    assert_eq!(control_result.dispositions.len(), 1);
    assert_eq!(control_result.refused, None);
    assert_eq!(control_consumer.try_pop().unwrap(), command(17));
    queue_empty(&mut control_consumer);
    let mut expected = control_stats;
    expected.alloc.attempted += 2;
    expected.alloc.succeeded += 2;
    expected.alloc.requested_bytes += 62;
    expected.dealloc.attempted += 2;
    expected.dealloc.succeeded += 2;
    expected.dealloc.requested_bytes += 62;
    assert_eq!(
        stats, expected,
        "injected callback requests must be inside the measured public call"
    );
    raw(
        "runtime.mapper.same-domain-control",
        "normalized_metadata same_business_transition clock=Disabled",
        control_stats,
    );
    raw(
        "runtime.mapper.allocating",
        "domains=9->1,8->2 map2quality2 callback_payload31x2 clock=Disabled",
        stats,
    );
}
