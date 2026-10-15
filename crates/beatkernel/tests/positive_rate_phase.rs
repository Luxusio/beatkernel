//! Independent contracts for the shared retained-playback phase.
//!
//! Literal odd-frame oracles and a small fixed-grid model deliberately avoid
//! the production phase's mixed-fraction arithmetic and inverse algorithm.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

use beatkernel::{
    time::{Duration, Timestamp},
    transport::{PositiveRatePhase, PositiveRatePhaseError as Error, Rate},
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

// SAFETY: Each operation forwards unchanged allocation arguments to System;
// bookkeeping touches only allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc's caller provides a valid layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        // SAFETY: GlobalAlloc's caller provides a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        // SAFETY: The caller supplies a live System allocation and valid size.
        unsafe { System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(2);
        // SAFETY: The caller supplies the live pointer and original layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn track<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|track| track.set(true));
    let result = operation();
    TRACK.with(|track| track.set(false));
    (result, COUNTS.with(Cell::get))
}

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}

fn ns(value: i64) -> Duration {
    Duration::from_nanos(value)
}

fn rate(numerator: i64, denominator: u64) -> Rate {
    Rate::new(numerator, denominator).unwrap()
}

fn phase(start: i64, preroll: i64, speed: Rate) -> PositiveRatePhase {
    PositiveRatePhase::new(ts(start), ns(preroll), speed, None).unwrap()
}

#[test]
fn phase_is_copy_equatable_and_reports_normalized_rate_and_segment_base() {
    fn assert_traits<T: Copy + Eq + std::fmt::Debug>() {}
    assert_traits::<PositiveRatePhase>();
    let original = phase(100, 3, rate(6, 4));
    let copy = original;
    assert_eq!(original, copy);
    assert_eq!(original.rate(), rate(3, 2));
    assert_eq!(original.base_elapsed(), 0);
    let changed = original.with_rate_at_elapsed(7, rate(2, 3)).unwrap();
    assert_eq!(changed.rate(), rate(2, 3));
    assert_eq!(changed.base_elapsed(), 7);
    assert_eq!(original.rate(), rate(3, 2));
    assert_eq!(original.base_elapsed(), 0);
}

#[test]
fn original_start_stays_outside_the_signed_truncation() {
    for start in [i64::MIN, -100, 0, 100, i64::MAX] {
        let owner = phase(start, 1, rate(1, 2));
        assert_eq!(owner.song_at_elapsed(0), Ok(ts(start)));
        assert_eq!(owner.song_at_elapsed(1), Ok(ts(start)));
    }
    let owner = phase(100, 3, rate(3, 2));
    for (elapsed, song) in [(0, 96), (1, 97), (2, 99), (3, 100), (4, 101)] {
        assert_eq!(owner.song_at_elapsed(elapsed), Ok(ts(song)));
    }
}

#[test]
fn exact_original_song_phase_is_floor_normalized_and_distinct_from_display() {
    let owner = phase(100, 1, rate(1, 2));
    assert_eq!(owner.song_at_elapsed(0), Ok(ts(100)));
    assert_eq!(owner.original_song_phase_at_elapsed(0), Ok((99, 1, 2)));
    assert_eq!(owner.original_song_phase_at_elapsed(1), Ok((100, 0, 1)));
    assert_eq!(owner.original_song_phase_at_elapsed(2), Ok((100, 1, 2)));
    let changed = owner.with_rate_at_elapsed(2, rate(3, 2)).unwrap();
    assert_eq!(changed.original_song_phase_at_elapsed(2), Ok((100, 1, 2)));
    assert_eq!(changed.original_song_phase_at_elapsed(3), Ok((102, 0, 1)));

    let negative = phase(0, 1, rate(1, 2));
    assert_eq!(negative.original_song_phase_at_elapsed(0), Ok((-1, 1, 2)));
    let thirds = negative.with_rate_at_elapsed(0, rate(1, 3)).unwrap();
    assert_eq!(thirds.original_song_phase_at_elapsed(1), Ok((-1, 5, 6)));
    assert_eq!(thirds.original_song_phase_at_elapsed(2), Ok((0, 1, 6)));
    assert_eq!(
        phase(i64::MIN, 1, rate(1, 2)).original_song_phase_at_elapsed(0),
        Ok((i128::from(i64::MIN) - 1, 1, 2))
    );
}

#[test]
fn exact_original_song_accessor_exposes_the_preserved_odd_frame_residue() {
    let first = phase(0, 3, rate(2, 3));
    assert_eq!(
        first.original_song_phase_at_elapsed(22_675),
        Ok((15_114, 2, 3))
    );
    let changed = first.with_rate_at_elapsed(22_675, rate(1, 2)).unwrap();
    assert_eq!(
        changed.original_song_phase_at_elapsed(90_702),
        Ok((49_128, 1, 6))
    );

    let second = phase(0, 3, rate(1, 2));
    assert_eq!(
        second.original_song_phase_at_elapsed(41_666),
        Ok((20_831, 1, 2))
    );
    let changed = second.with_rate_at_elapsed(41_666, rate(3, 2)).unwrap();
    assert_eq!(
        changed.original_song_phase_at_elapsed(83_333),
        Ok((83_332, 0, 1))
    );
}

#[test]
fn exact_original_song_accessor_is_uncapped_wide_and_checks_chronology() {
    let owner = PositiveRatePhase::new(ts(100), ns(3), rate(1, 2), Some(ts(102))).unwrap();
    assert_eq!(owner.song_at_elapsed(100), Ok(ts(102)));
    assert_eq!(owner.original_song_phase_at_elapsed(100), Ok((148, 1, 2)));
    let changed = owner.with_rate_at_elapsed(8, rate(3, 2)).unwrap();
    assert_eq!(
        changed.original_song_phase_at_elapsed(-1),
        Err(Error::NegativeElapsed)
    );
    assert_eq!(
        changed.original_song_phase_at_elapsed(7),
        Err(Error::RegressingElapsed)
    );

    let wide = phase(i64::MAX, 0, Rate::NORMAL);
    assert_eq!(wide.song_at_elapsed(1), Err(Error::Overflow));
    assert_eq!(
        wide.original_song_phase_at_elapsed(1),
        Ok((i128::from(i64::MAX) + 1, 0, 1))
    );
    let capped = PositiveRatePhase::new(ts(0), ns(0), rate(i64::MAX, 1), Some(ts(1))).unwrap();
    assert_eq!(capped.song_at_elapsed(i128::MAX), Ok(ts(1)));
    assert_eq!(
        capped.original_song_phase_at_elapsed(i128::MAX),
        Err(Error::Overflow)
    );
}

#[test]
fn odd_44100_hz_cut_retains_the_two_thirds_residue() {
    let original = phase(0, 3, rate(2, 3));
    assert_eq!(original.song_at_elapsed(22_675), Ok(ts(15_114)));
    let changed = original.with_rate_at_elapsed(22_675, rate(1, 2)).unwrap();
    assert_eq!(changed.song_at_elapsed(22_675), Ok(ts(15_114)));
    for (elapsed, song) in [(45_351, 26_452), (68_027, 37_790), (90_702, 49_128)] {
        assert_eq!(changed.song_at_elapsed(elapsed), Ok(ts(song)));
    }
    assert_eq!(original.song_at_elapsed(90_702), Ok(ts(60_466)));
}

#[test]
fn odd_48000_hz_cut_retains_the_half_nanosecond() {
    let original = phase(0, 3, rate(1, 2));
    assert_eq!(original.song_at_elapsed(41_666), Ok(ts(20_831)));
    let changed = original.with_rate_at_elapsed(41_666, rate(3, 2)).unwrap();
    for (elapsed, song) in [(62_500, 52_082), (83_333, 83_332), (104_166, 114_581)] {
        assert_eq!(changed.song_at_elapsed(elapsed), Ok(ts(song)));
    }
}

#[test]
fn a_sub_nanosecond_cut_is_not_reanchored_at_the_integer_song() {
    let original = phase(0, 1, rate(1, 2));
    assert_eq!(original.song_at_elapsed(2), Ok(ts(0)));
    let changed = original.with_rate_at_elapsed(2, rate(3, 2)).unwrap();
    assert_eq!(changed.song_at_elapsed(2), Ok(ts(0)));
    assert_eq!(changed.song_at_elapsed(3), Ok(ts(2)));
    assert_eq!(changed.elapsed_at_original_song(ts(1)), Ok(3));
    assert_eq!(changed.elapsed_at_original_song(ts(2)), Ok(3));
}

#[test]
fn normalized_same_rate_is_an_exact_noop_even_at_a_later_elapsed() {
    let original = phase(37, 3, rate(3, 2));
    assert_eq!(original.with_rate_at_elapsed(17, rate(6, 4)), Ok(original));
    let changed = original.with_rate_at_elapsed(5, rate(2, 3)).unwrap();
    assert_eq!(changed.with_rate_at_elapsed(19, rate(10, 15)), Ok(changed));
    assert_eq!(changed.base_elapsed(), 5);
}

#[test]
fn setup_and_query_errors_are_typed_and_candidates_do_not_mutate() {
    assert_eq!(
        PositiveRatePhase::new(ts(0), ns(-1), Rate::NORMAL, None),
        Err(Error::NegativePreroll)
    );
    for speed in [Rate::ZERO, Rate::REVERSE, rate(i64::MIN, 7)] {
        assert_eq!(
            PositiveRatePhase::new(ts(0), ns(0), speed, None),
            Err(Error::NonPositiveRate)
        );
    }
    assert_eq!(
        PositiveRatePhase::new(ts(10), ns(0), Rate::NORMAL, Some(ts(9))),
        Err(Error::InvalidEnd)
    );
    assert_eq!(
        PositiveRatePhase::new(ts(i64::MIN), ns(1), Rate::NORMAL, Some(ts(0))),
        Err(Error::Overflow)
    );
    assert_eq!(
        PositiveRatePhase::new(ts(0), ns(i64::MAX), rate(i64::MAX, 1), Some(ts(1))),
        Err(Error::Overflow)
    );

    let original = phase(0, 1, rate(1, 2));
    let changed = original.with_rate_at_elapsed(10, Rate::NORMAL).unwrap();
    let copy = changed;
    assert_eq!(changed.song_at_elapsed(-1), Err(Error::NegativeElapsed));
    assert_eq!(changed.song_at_elapsed(9), Err(Error::RegressingElapsed));
    assert_eq!(
        changed.with_rate_at_elapsed(-1, rate(2, 1)),
        Err(Error::NegativeElapsed)
    );
    assert_eq!(
        changed.with_rate_at_elapsed(9, Rate::NORMAL),
        Err(Error::RegressingElapsed)
    );
    for speed in [Rate::ZERO, Rate::REVERSE] {
        assert_eq!(
            changed.with_rate_at_elapsed(10, speed),
            Err(Error::NonPositiveRate)
        );
    }
    assert_eq!(changed, copy);
}

#[test]
fn inverse_preserves_exact_preroll_onset_and_negative_target_strictness() {
    let fractional = phase(100, 3, rate(1, 2));
    assert_eq!(fractional.elapsed_at_original_song(ts(99)), Ok(0));
    // The displayed integer already equals S at E=2, but BGM onset is E=3.
    assert_eq!(fractional.song_at_elapsed(2), Ok(ts(100)));
    assert_eq!(fractional.elapsed_at_original_song(ts(100)), Ok(3));
    assert_eq!(fractional.elapsed_at_original_song(ts(101)), Ok(5));

    let exact = phase(100, 4, rate(1, 2));
    assert_eq!(exact.elapsed_at_original_song(ts(98)), Ok(0));
    // Q=-2 at E=0: target S-1 needs Q>-2, so equality is insufficient.
    assert_eq!(exact.elapsed_at_original_song(ts(99)), Ok(1));
    assert_eq!(exact.elapsed_at_original_song(ts(100)), Ok(4));
    let changed = exact.with_rate_at_elapsed(1, rate(2, 3)).unwrap();
    assert_eq!(changed.elapsed_at_original_song(ts(98)), Ok(1));
    assert_eq!(changed.elapsed_at_original_song(ts(99)), Ok(1));
    assert_eq!(changed.elapsed_at_original_song(ts(100)), Ok(4));
    assert_eq!(changed.elapsed_at_original_song(ts(101)), Ok(5));
}

#[test]
fn inverse_inclusive_equality_and_nonexact_crossings_use_the_first_integer_e() {
    let owner = phase(0, 0, rate(2, 3));
    for (target, elapsed) in [(0, 0), (1, 2), (2, 3), (3, 5), (4, 6)] {
        assert_eq!(owner.elapsed_at_original_song(ts(target)), Ok(elapsed));
    }
    let changed = owner.with_rate_at_elapsed(1, rate(3, 2)).unwrap();
    for (target, elapsed) in [(0, 1), (1, 2), (2, 2), (3, 3), (4, 4)] {
        assert_eq!(changed.elapsed_at_original_song(ts(target)), Ok(elapsed));
    }
}

#[test]
fn finite_end_caps_only_song_queries_and_does_not_destroy_phase_or_inverse() {
    let owner = PositiveRatePhase::new(ts(100), ns(3), rate(1, 2), Some(ts(102))).unwrap();
    for (elapsed, song) in [(0, 99), (2, 100), (7, 102), (100, 102)] {
        assert_eq!(owner.song_at_elapsed(elapsed), Ok(ts(song)));
    }
    assert_eq!(owner.elapsed_at_original_song(ts(103)), Ok(9));
    let changed = owner.with_rate_at_elapsed(8, rate(3, 2)).unwrap();
    assert_eq!(changed.song_at_elapsed(8), Ok(ts(102)));
    assert_eq!(changed.song_at_elapsed(9), Ok(ts(102)));
    assert_eq!(changed.elapsed_at_original_song(ts(101)), Ok(8));
    assert_eq!(changed.elapsed_at_original_song(ts(103)), Ok(9));
    let empty = PositiveRatePhase::new(ts(100), ns(1), rate(1, 2), Some(ts(100))).unwrap();
    assert_eq!(empty.song_at_elapsed(0), Ok(ts(100)));
    assert_eq!(empty.song_at_elapsed(i128::MAX), Ok(ts(100)));
}

#[test]
fn finite_end_saturates_positive_wide_overflow_but_real_change_needs_full_phase() {
    let owner = PositiveRatePhase::new(ts(0), ns(0), rate(i64::MAX, 1), Some(ts(1))).unwrap();
    assert_eq!(owner.song_at_elapsed(i128::MAX), Ok(ts(1)));
    assert_eq!(
        owner.with_rate_at_elapsed(i128::MAX, rate(1, 2)),
        Err(Error::Overflow)
    );
    assert_eq!(owner.song_at_elapsed(0), Ok(ts(0)));
    assert_eq!(
        phase(0, 0, rate(i64::MAX, 1)).song_at_elapsed(i128::MAX),
        Err(Error::Overflow)
    );
}

#[test]
fn denominator_capacity_refuses_without_mutation_but_a_cancelled_residue_fits() {
    let original = phase(0, 1, rate(1, u64::MAX));
    assert_eq!(
        original.with_rate_at_elapsed(0, rate(1, 2)),
        Err(Error::DenominatorCapacity)
    );
    assert_eq!(original, phase(0, 1, rate(1, u64::MAX)));
    // At E=1, -1/MAX + 1/MAX cancels exactly. Do not reject using old D alone.
    let changed = original.with_rate_at_elapsed(1, rate(1, 2)).unwrap();
    assert_eq!(changed.song_at_elapsed(1), Ok(ts(0)));
    assert_eq!(changed.song_at_elapsed(3), Ok(ts(1)));
    assert_eq!(changed.elapsed_at_original_song(ts(1)), Ok(3));

    let almost_half = phase(0, 0, rate(i64::MAX, u64::MAX));
    assert_eq!(almost_half.song_at_elapsed(2), Ok(ts(0)));
    assert_eq!(almost_half.song_at_elapsed(3), Ok(ts(1)));
    assert_eq!(almost_half.elapsed_at_original_song(ts(1)), Ok(3));
}

#[test]
fn wide_products_are_decomposed_when_the_exact_song_and_inverse_still_fit() {
    // (target-S)*D exceeds i128, but division by N leaves this exact answer.
    let owner = phase(i64::MIN, 0, rate(i64::MAX, u64::MAX));
    let elapsed = 36_893_488_147_419_103_233_i128;
    assert!(elapsed.checked_mul(i128::from(i64::MAX)).is_none());
    assert_eq!(owner.elapsed_at_original_song(ts(i64::MAX)), Ok(elapsed));
    assert_eq!(owner.song_at_elapsed(elapsed - 1), Ok(ts(i64::MAX - 1)));
    assert_eq!(owner.song_at_elapsed(elapsed), Ok(ts(i64::MAX)));

    let slow = phase(0, 0, rate(1, u64::MAX));
    let last = 170_141_183_460_469_231_704_017_187_605_319_778_305_i128;
    assert_eq!(slow.elapsed_at_original_song(ts(i64::MAX)), Ok(last));
    assert_eq!(slow.song_at_elapsed(last), Ok(ts(i64::MAX)));
}

#[test]
fn a_genuinely_unrepresentable_elapsed_or_song_reports_overflow() {
    let slow = phase(i64::MIN, 0, rate(1, u64::MAX));
    assert_eq!(
        slow.elapsed_at_original_song(ts(i64::MAX)),
        Err(Error::Overflow)
    );
    assert_eq!(
        phase(i64::MAX, 0, Rate::NORMAL).song_at_elapsed(1),
        Err(Error::Overflow)
    );
    let original = phase(0, 0, Rate::NORMAL);
    assert_eq!(
        original.with_rate_at_elapsed(i128::MAX, rate(1, 2)),
        Err(Error::Overflow)
    );
    assert_eq!(original.song_at_elapsed(0), Ok(ts(0)));
}

// All test rates divide this grid. The independent model keeps one small
// integer numerator and locates inverse crossings by enumeration, never by
// reproducing production's mixed-fraction arithmetic or quotient decomposition.
const GRID: i128 = 30;
const SPEEDS: [(i64, u64); 5] = [(1, 2), (2, 3), (3, 2), (5, 3), (7, 5)];

#[derive(Clone, Copy)]
struct Model {
    start: i64,
    base: i128,
    phase_units: i128,
    speed: (i64, u64),
    end: Option<i64>,
}

impl Model {
    fn units_at(self, elapsed: i128) -> i128 {
        self.phase_units
            + (elapsed - self.base) * i128::from(self.speed.0) * (GRID / i128::from(self.speed.1))
    }

    fn song_at(self, elapsed: i128) -> Timestamp {
        let song = i128::from(self.start) + self.units_at(elapsed) / GRID;
        ts(i64::try_from(self.end.map_or(song, |end| song.min(i128::from(end)))).unwrap())
    }

    fn inverse(self, target: i64) -> i128 {
        let relative = i128::from(target) - i128::from(self.start);
        (self.base..=self.base + 128)
            .find(|&elapsed| {
                if relative >= 0 {
                    self.units_at(elapsed) >= relative * GRID
                } else {
                    self.units_at(elapsed) > (relative - 1) * GRID
                }
            })
            .expect("bounded model reaches each test target")
    }

    fn changed(self, elapsed: i128, speed: (i64, u64)) -> Self {
        if self.speed == speed {
            self
        } else {
            Self {
                base: elapsed,
                phase_units: self.units_at(elapsed),
                speed,
                ..self
            }
        }
    }
}

fn check_model(owner: PositiveRatePhase, model: Model) {
    assert_eq!(owner.base_elapsed(), model.base);
    assert_eq!(owner.rate(), rate(model.speed.0, model.speed.1));
    for elapsed in model.base..=model.base + 8 {
        assert_eq!(owner.song_at_elapsed(elapsed), Ok(model.song_at(elapsed)));
        let original_units = i128::from(model.start) * GRID + model.units_at(elapsed);
        let remainder = original_units.rem_euclid(GRID);
        let common = [30, 15, 10, 6, 5, 3, 2, 1]
            .into_iter()
            .find(|divisor| remainder % divisor == 0)
            .unwrap();
        assert_eq!(
            owner.original_song_phase_at_elapsed(elapsed),
            Ok((
                original_units.div_euclid(GRID),
                (remainder / common) as u64,
                (GRID / common) as u64,
            ))
        );
    }
    for target in model.start - 4..=model.start + 12 {
        assert_eq!(
            owner.elapsed_at_original_song(ts(target)),
            Ok(model.inverse(target))
        );
    }
}

#[test]
fn bounded_cut_chains_match_an_independent_fixed_grid_and_enumerated_inverse() {
    for start in [-100, 100] {
        for preroll in 0..=3 {
            for initial in SPEEDS {
                for second in SPEEDS {
                    for third in SPEEDS {
                        for first_cut in [0, 1, 4] {
                            let end = (first_cut % 2 == 0).then_some(start + 5);
                            let original = PositiveRatePhase::new(
                                ts(start),
                                ns(preroll),
                                rate(initial.0, initial.1),
                                end.map(ts),
                            )
                            .unwrap();
                            let model = Model {
                                start,
                                base: 0,
                                phase_units: -i128::from(preroll)
                                    * i128::from(initial.0)
                                    * (GRID / i128::from(initial.1)),
                                speed: initial,
                                end,
                            };
                            check_model(original, model);
                            let changed = original
                                .with_rate_at_elapsed(first_cut, rate(second.0, second.1))
                                .unwrap();
                            let changed_model = model.changed(first_cut, second);
                            check_model(changed, changed_model);
                            let last_cut = first_cut + 3;
                            let last = changed
                                .with_rate_at_elapsed(last_cut, rate(third.0, third.1))
                                .unwrap();
                            check_model(last, changed_model.changed(last_cut, third));
                            // Earlier immutable segments retain their own mapping.
                            check_model(original, model);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn calibrated_setup_query_inverse_and_change_paths_allocate_and_free_nothing() {
    let (_, calibration) = track(|| {
        let layout = Layout::from_size_align(8, 8).unwrap();
        // SAFETY: Both layouts are valid; the returned allocation is checked,
        // resized once and released using its matching new layout.
        unsafe {
            let pointer = std::alloc::alloc(layout);
            assert!(!pointer.is_null());
            black_box(pointer).write(17);
            let resized = std::alloc::realloc(pointer, layout, 16);
            assert!(!resized.is_null());
            black_box(resized).write(23);
            std::alloc::dealloc(resized, Layout::from_size_align(16, 8).unwrap());
        }
    });
    assert_eq!(calibration, [1, 1, 1]);

    let (_, counts) = track(|| {
        let mut owner = phase(100, 3, rate(2, 3));
        for step in 0..1_000_i128 {
            let elapsed = step * 3;
            let speed = if step % 2 == 0 {
                rate(1, 2)
            } else {
                rate(2, 3)
            };
            owner = owner
                .with_rate_at_elapsed(black_box(elapsed), speed)
                .unwrap();
            black_box(owner.song_at_elapsed(elapsed + 1).unwrap());
            black_box(owner.original_song_phase_at_elapsed(elapsed + 1).unwrap());
            black_box(owner.elapsed_at_original_song(ts(10_000)).unwrap());
            black_box(owner.with_rate_at_elapsed(elapsed + 2, speed).unwrap());
        }
        black_box(owner);
    });
    assert_eq!(counts, [0, 0, 0]);
}
