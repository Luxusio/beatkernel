//! Exact mathematical ordering grants no native presentation authority.
use beatkernel::{
    audio::{AudioError, InterpolatedTargetTime, TargetTime},
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    cmp::Ordering,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<[usize; 3]> = const { Cell::new([0; 3]) };
}
struct Allocator;
fn count(index: usize) {
    let _ = TRACK.try_with(|active| {
        if active.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut value = counts.get();
                value[index] += 1;
                counts.set(value);
            });
        }
    });
}
// SAFETY: Pointer and layout arguments pass unchanged to System. Bookkeeping
// touches only allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(2);
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(operation: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|counts| counts.set([0; 3]));
    TRACK.with(|active| active.set(true));
    let value = operation();
    TRACK.with(|active| active.set(false));
    (value, COUNTS.with(Cell::get))
}
fn time(numerator: u64, denominator: u64) -> TargetTime {
    TargetTime::new(0, numerator, denominator).unwrap()
}
fn query(
    base: TargetTime,
    left: u64,
    right: u64,
    frequency: u64,
    offset: i128,
    span: u64,
) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(base, left, right, frequency, offset, span).unwrap()
}
fn constant(base: TargetTime) -> InterpolatedTargetTime {
    query(base, 0, 0, 1, 0, 1)
}
fn origin() -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(91),
        timestamp: Timestamp::ZERO,
    }
}
fn assert_order(left: InterpolatedTargetTime, right: InterpolatedTargetTime, expected: Ordering) {
    assert_eq!(left.compare_interpolated_time(right), expected);
    assert_eq!(right.compare_interpolated_time(left), expected.reverse());
    assert_eq!(left.compare_interpolated_time(left), Ordering::Equal);
    assert_eq!(right.compare_interpolated_time(right), Ordering::Equal);
}

#[test]
fn equal_rounded_points_can_have_different_active_cut_elapsed() {
    // 1/3 + 22/15e9 = 333333334.8ns;
    // 1/3 + 13/15e9 = 333333334.2ns.
    let later = query(time(1, 3), 0, 22, 3_000_000_000, 1, 5);
    let earlier = query(time(1, 3), 0, 13, 3_000_000_000, 1, 5);
    assert_eq!(
        later.point(origin()).unwrap().timestamp,
        Timestamp::from_nanos(333_333_334)
    );
    assert_eq!(
        earlier.point(origin()).unwrap(),
        later.point(origin()).unwrap()
    );
    assert_eq!(later.elapsed_nanos_since_source_frame(1, 3), Ok(1));
    assert_eq!(earlier.elapsed_nanos_since_source_frame(1, 3), Ok(0));
    assert_order(later, earlier, Ordering::Greater);
}

#[test]
fn different_base_counter_and_drift_decompositions_are_mathematically_equal() {
    let half = constant(time(1, 2));
    let base_and_counter = query(time(1, 3), 1, 1, 6, -17, 19);
    let base_and_drift = query(time(1, 4), 0, 1, 2, 1, 2);
    let counter_and_negative_drift = query(time(0, 1), 2, 3, 3, -1, 2);
    for equivalent in [base_and_counter, base_and_drift, counter_and_negative_drift] {
        assert_order(half, equivalent, Ordering::Equal);
    }
}

#[test]
fn signed_host_interpolation_orders_negative_zero_and_positive_durations() {
    let zero = constant(time(0, 1));
    let negative = query(time(0, 1), 0, 1, 3, -1, 2);
    let positive = query(time(0, 1), 0, 1, 3, 1, 2);
    assert_order(negative, zero, Ordering::Less);
    assert_order(zero, positive, Ordering::Less);
    assert_order(negative, positive, Ordering::Less);
    assert_eq!(
        negative.point(origin()).unwrap().timestamp,
        Timestamp::from_nanos(-166_666_667)
    );
    assert_eq!(
        positive.point(origin()).unwrap().timestamp,
        Timestamp::from_nanos(166_666_666)
    );
}

#[test]
fn public_constructor_signed_whole_extremes_do_not_overflow_ordering() {
    let k = 1u64 << 63;
    let base = TargetTime::new(k - 1, 0, 1).unwrap();
    // (K-1) - (K+1)(2K-1) = -2K² = i128::MIN.
    let minimum = query(base, 0, k + 1, 1, -i128::from(u64::MAX), 1);
    // (K-1) + K(2K-1) = 2K²-1 = i128::MAX.
    let maximum = query(base, 0, k, 1, i128::from(u64::MAX), 1);
    let zero = constant(time(0, 1));
    assert_order(minimum, maximum, Ordering::Less);
    assert_order(minimum, zero, Ordering::Less);
    assert_order(zero, maximum, Ordering::Less);
    assert_eq!(minimum.point(origin()), Err(AudioError::Overflow));
    assert_eq!(maximum.point(origin()), Err(AudioError::Overflow));
    // Same extremes assembled differently exercise equality near both limits.
    let minimum_fractional_base = query(
        TargetTime::new(k - 1, 1, 2).unwrap(),
        0,
        k + 1,
        1,
        -i128::from(u64::MAX),
        1,
    );
    let maximum_fractional_base = query(
        TargetTime::new(k - 1, 1, 2).unwrap(),
        0,
        k,
        1,
        i128::from(u64::MAX),
        1,
    );
    assert_order(minimum, minimum_fractional_base, Ordering::Less);
    assert_order(maximum, maximum_fractional_base, Ordering::Less);
}

fn near_two(b: u64, f: u64, h: u64) -> InterpolatedTargetTime {
    // Exact identity: (B-1)/B + (F-1)/F + (H-1)/(F H)
    // = 2 - 1/B - 1/(F H).
    query(time(b - 1, b), f - 1, f, f, i128::from(h - 1), h)
}

#[test]
fn six_mixed_wide_denominators_compare_without_narrowing_or_rounding() {
    let m = u64::MAX;
    let later = near_two(m, m - 2, m - 4);
    let earlier = near_two(m - 1, m - 3, m - 5);
    // Each positive deficit of later is strictly smaller: B and F*H
    // are both larger. All six input denominator widths are near 64/128 bits.
    assert_order(later, earlier, Ordering::Greater);
    assert_eq!(
        later.point(origin()).unwrap().timestamp,
        Timestamp::from_nanos(1_999_999_999)
    );
    assert_eq!(
        earlier.point(origin()).unwrap(),
        later.point(origin()).unwrap()
    );
    assert_order(
        later,
        constant(TargetTime::new(2, 0, 1).unwrap()),
        Ordering::Less,
    );
}

#[test]
fn maximum_frequency_and_span_retain_a_tiny_deficit_from_one() {
    let m = u64::MAX;
    // (M-1)/M + (M-1)/(M*M) = 1 - 1/(M*M).
    let below = query(time(0, 1), m - 1, m, m, i128::from(m - 1), m);
    let one = constant(TargetTime::new(1, 0, 1).unwrap());
    let below_by_tick = query(time(0, 1), m - 1, m - 1, m, 0, m);
    assert_order(below_by_tick, below, Ordering::Less);
    assert_order(below, one, Ordering::Less);
    assert_eq!(
        below.point(origin()).unwrap().timestamp,
        Timestamp::from_nanos(999_999_999)
    );
}

#[test]
fn wide_denominator_signed_cancellation_distinguishes_both_sides_of_zero() {
    let m = u64::MAX;
    // 1/M - 1/(M*M) is positive; -1/(M*M) is negative.
    let positive = query(time(1, m), 0, 1, m, -1, m);
    let negative = query(time(0, 1), 0, 1, m, -1, m);
    let zero = constant(time(0, 1));
    assert_order(negative, zero, Ordering::Less);
    assert_order(zero, positive, Ordering::Less);
    assert_order(negative, positive, Ordering::Less);
    assert_eq!(positive.point(origin()).unwrap().timestamp, Timestamp::ZERO);
    assert_eq!(
        negative.point(origin()).unwrap().timestamp,
        Timestamp::from_nanos(-1)
    );
}

#[test]
fn signed_small_grid_matches_independent_cross_multiplied_rational_oracle() {
    for f in 1..=5u64 {
        for h in 1..=5u64 {
            for offset in -4..=6i128 {
                let left = query(time(1, 7), 2, 5, f, offset, h);
                // Q = (F H + 14 H + 21 offset)/(7 F H).
                let left_n = i128::from(f * h + 14 * h) + 21 * offset;
                let left_d = i128::from(7 * f * h);
                for other_offset in -3..=5i128 {
                    let right = query(time(2, 11), 1, 3, h, other_offset, f);
                    // R = (2 H F + 11 F + 22 other_offset)/(11 H F).
                    let right_n = i128::from(2 * h * f + 11 * f) + 22 * other_offset;
                    let right_d = i128::from(11 * h * f);
                    assert_order(left, right, (left_n * right_d).cmp(&(right_n * left_d)));
                }
            }
        }
    }
}

#[test]
fn exact_order_is_transitive_for_dense_same_floor_and_wide_families() {
    let m = u64::MAX;
    let dense = [1, 3, 7, 13, 22, 29].map(|right| query(time(1, 3), 0, right, 3_000_000_000, 1, 5));
    let wide = [5, 4, 3, 2, 1, 0].map(|gap| near_two(m - gap, m - gap, m - gap));
    for family in [dense, wide] {
        for i in 0..family.len() {
            for j in i + 1..family.len() {
                assert_order(family[i], family[j], Ordering::Less);
                for k in j + 1..family.len() {
                    assert_eq!(
                        family[i].compare_interpolated_time(family[k]),
                        Ordering::Less
                    );
                    assert_eq!(
                        family[j].compare_interpolated_time(family[k]),
                        Ordering::Less
                    );
                }
            }
        }
    }
}

#[test]
fn existing_target_elapsed_and_ahead_comparisons_remain_exact() {
    let q = query(time(1, 3), 0, 22, 3_000_000_000, 1, 5);
    assert_eq!(
        q.compare_target_time(time(333_333_334, 1_000_000_000)),
        Ordering::Greater
    );
    assert_eq!(q.elapsed_nanos_since_source_frame(1, 3), Ok(1));
    assert_eq!(
        q.compare_native_time_with_offset(time(1, 3), 0, 3_000_000_000, 1),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        q.compare_native_time_with_offset(time(1, 3), 0, 3_000_000_000, 2),
        Ok(Ordering::Less)
    );
}

#[test]
fn allocation_probe_calibrates_alloc_realloc_and_dealloc() {
    let (_, counts) = tracked(|| unsafe {
        let layout = Layout::from_size_align(8, 8).unwrap();
        let pointer = std::alloc::alloc(layout);
        assert!(!pointer.is_null());
        let resized = std::alloc::realloc(pointer, layout, 16);
        assert!(!resized.is_null());
        std::alloc::dealloc(resized, Layout::from_size_align(16, 8).unwrap());
    });
    assert_eq!(counts, [1, 1, 1]);
}

#[test]
fn actual_hot_wide_and_signed_comparisons_allocate_nothing() {
    let m = u64::MAX;
    let wide = near_two(m, m - 2, m - 4);
    let smaller = near_two(m - 1, m - 3, m - 5);
    let k = 1u64 << 63;
    let base = TargetTime::new(k - 1, 0, 1).unwrap();
    let minimum = query(base, 0, k + 1, 1, -i128::from(m), 1);
    let maximum = query(base, 0, k, 1, i128::from(m), 1);
    let (orders, counts) = tracked(|| {
        let mut orders = [Ordering::Equal; 4];
        for _ in 0..64 {
            orders = [
                std::hint::black_box(wide).compare_interpolated_time(std::hint::black_box(smaller)),
                std::hint::black_box(smaller).compare_interpolated_time(std::hint::black_box(wide)),
                std::hint::black_box(wide).compare_interpolated_time(std::hint::black_box(wide)),
                std::hint::black_box(minimum)
                    .compare_interpolated_time(std::hint::black_box(maximum)),
            ];
            std::hint::black_box(orders);
        }
        orders
    });
    assert_eq!(
        orders,
        [
            Ordering::Greater,
            Ordering::Less,
            Ordering::Equal,
            Ordering::Less
        ]
    );
    assert_eq!(counts, [0, 0, 0]);
}
