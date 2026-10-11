//! Allocation requests for named gauge work; preparation stays outside counting.
use beatkernel::{
    chart::ObjectId,
    input::GameControlId,
    judge::{
        HazardEvent, HazardId, HazardOutcome, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage,
        MissReason,
    },
    time::{Duration, Timestamp},
};
use beatkernel_bms_runtime::gauge::{
    BmsGauge, GaugeDynamics, GaugeError, GaugeFailure, GaugeProfile, GradeDelta, MAX_GAUGE_GRADES,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
        // SAFETY: Caller supplies a live System allocation and valid size.
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            record(size);
        }
        result
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: Caller supplies the original allocation pointer/layout.
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
fn hit(grade: u32, stage: JudgeStage) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(1),
        stage,
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(grade),
            delta: Duration::ZERO,
        },
        at: Timestamp::ZERO,
        input: None,
    }
}
fn hazard(value: u64, outcome: HazardOutcome) -> HazardEvent {
    HazardEvent {
        id: HazardId(1),
        at: Timestamp::ZERO,
        control: GameControlId(1),
        value,
        outcome,
        input: None,
    }
}
fn profile(grades: usize) -> GaugeProfile {
    let entries = (0..grades)
        .rev()
        .map(|index| GradeDelta {
            grade: JudgeGrade(u32::MAX - index as u32),
            delta: index as i64 + 1,
        })
        .collect();
    GaugeProfile::new(20_000_000, 80_000_000, 17, -10_000_000, true, entries)
        .unwrap()
        .with_dynamics(GaugeDynamics {
            minimum_alive: 0,
            failure_below: 2_000_000,
            damage_reduction_below: 32_000_000,
        })
        .unwrap()
}
fn assert_zero(stats: Requests) {
    assert_eq!(stats, Requests::default());
}

#[test]
fn observe_has_no_requests_for_default_and_every_supported_custom_grade_count() {
    for grades in 0..=MAX_GAUGE_GRADES {
        for default in [false, true] {
            if default && grades != 0 {
                continue;
            }
            let policy = if default {
                GaugeProfile::default()
            } else {
                profile(grades)
            };
            for count in [0, 1, 128, 129] {
                let events: Vec<_> = (0..count)
                    .map(|index| {
                        hit(
                            if index % 2 == 0 { u32::MAX } else { 7 },
                            match index % 3 {
                                0 => JudgeStage::Instant,
                                1 => JudgeStage::HoldHead,
                                _ => JudgeStage::HoldTail,
                            },
                        )
                    })
                    .collect();
                let mut gauge = BmsGauge::new(policy.clone());
                let (result, stats) = measure(|| gauge.observe(&events, &[]));
                result.unwrap();
                assert_zero(stats);
                let explicit = if grades == 0 {
                    policy.default_hit_delta()
                } else {
                    1
                };
                let expected = policy.initial_units()
                    + (count as u64).div_ceil(2) * explicit as u64
                    + (count as u64 / 2) * policy.default_hit_delta() as u64;
                assert_eq!(gauge.snapshot().level_units, expected.min(100_000_000));
                assert_eq!(gauge.profile(), &policy);
                println!("observe default={default} grades={grades} events={count}: {stats:?}");
            }
            let mut gauge = BmsGauge::new(policy.clone());
            let miss = JudgeEvent {
                outcome: JudgeOutcome::Miss {
                    reason: MissReason::HeadTimeout,
                },
                ..hit(7, JudgeStage::Instant)
            };
            let hazards = [
                hazard(1, HazardOutcome::Avoided),
                hazard(2, HazardOutcome::Triggered),
            ];
            let (result, stats) = measure(|| gauge.observe(&[miss], &hazards));
            result.unwrap();
            assert_zero(stats);
            assert_eq!(gauge.snapshot().level_units, 13_000_000);
            let fatal = [hazard(1295, HazardOutcome::Triggered)];
            let (result, stats) = measure(|| gauge.observe(&[], &fatal));
            result.unwrap();
            assert_zero(stats);
            assert_eq!(gauge.snapshot().failure, Some(GaugeFailure::InstantDeath));
            let before = gauge.clone();
            let (result, stats) =
                measure(|| gauge.observe(&[hit(u32::MAX, JudgeStage::Instant)], &hazards));
            result.unwrap();
            assert_zero(stats);
            assert_eq!(gauge, before);
            let mut hazard_only = BmsGauge::new(policy.clone());
            let (result, stats) = measure(|| hazard_only.observe(&[], &hazards));
            result.unwrap();
            assert_zero(stats);
            assert_eq!(hazard_only.snapshot().level_units, 19_000_000);
        }
    }
}

#[test]
fn rejected_hazards_are_allocation_free_and_atomic_even_after_failure() {
    for grades in [0, 1, MAX_GAUGE_GRADES] {
        for failed in [false, true] {
            for value in [0, 1296, u64::MAX] {
                for outcome in [HazardOutcome::Avoided, HazardOutcome::Triggered] {
                    let mut gauge = BmsGauge::new(profile(grades));
                    if failed {
                        gauge
                            .observe(&[], &[hazard(1295, HazardOutcome::Triggered)])
                            .unwrap();
                    }
                    let before = gauge.clone();
                    let events = [hit(u32::MAX, JudgeStage::HoldTail)];
                    let hazards = [hazard(1, HazardOutcome::Triggered), hazard(value, outcome)];
                    let (result, stats) = measure(|| gauge.observe(&events, &hazards));
                    assert_eq!(result, Err(GaugeError::InvalidDamage { value }));
                    assert_zero(stats);
                    assert_eq!(gauge, before);
                }
            }
        }
    }
}

#[test]
fn cold_clone_and_fallible_policy_association_request_only_owned_grade_storage() {
    for grades in 0..=MAX_GAUGE_GRADES {
        let policy = profile(grades);
        let gauge = BmsGauge::new(policy.clone());
        let expected = Requests {
            count: usize::from(grades != 0),
            total: grades * std::mem::size_of::<GradeDelta>(),
            largest: grades * std::mem::size_of::<GradeDelta>(),
        };
        let (cloned, stats) = measure(|| policy.clone());
        assert_eq!(stats, expected);
        assert_eq!(cloned, policy);
        let (copied, stats) = measure(|| policy.try_copy());
        let copied = copied.unwrap();
        assert_eq!(stats, expected);
        assert_eq!(copied, policy);
        let (cloned_gauge, stats) = measure(|| gauge.clone());
        assert_eq!(stats, expected);
        assert_eq!(cloned_gauge, gauge);
        if grades != 0 {
            assert_ne!(cloned.grades().as_ptr(), policy.grades().as_ptr());
            assert_ne!(copied.grades().as_ptr(), policy.grades().as_ptr());
            assert_ne!(
                cloned_gauge.profile().grades().as_ptr(),
                gauge.profile().grades().as_ptr()
            );
        }
        drop(policy);
        drop(gauge);
        assert_eq!(cloned, copied);
        assert_eq!(cloned_gauge.profile(), &copied);
        println!("cold-owned-policy grades={grades}: {stats:?}");
    }
    let default = GaugeProfile::default();
    let (cloned, stats) = measure(|| BmsGauge::default().clone());
    assert_eq!(cloned.profile(), &default);
    assert_zero(stats);
    let (copied, stats) = measure(|| default.try_copy());
    assert_eq!(copied.unwrap(), default);
    assert_zero(stats);
}

#[test]
fn unwind_disables_counting_for_the_next_operation() {
    let failed = std::panic::catch_unwind(|| measure(|| panic!("measurement failure")));
    assert!(failed.is_err());
    assert!(!ACTIVE.with(Cell::get));
    let (_, stats) = measure(|| ());
    assert_zero(stats);
}
