//! Exact signed HOST interpolation is mathematical data, not native admission.
use beatkernel::{
    audio::*,
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
// SAFETY: Arguments and returned allocation pointers pass unchanged to System.
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
fn tracked<T>(f: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|c| c.set([0; 3]));
    TRACK.with(|t| t.set(true));
    let value = f();
    TRACK.with(|t| t.set(false));
    (value, COUNTS.with(Cell::get))
}
fn time(n: u64, d: u64) -> TargetTime {
    TargetTime::new(0, n, d).unwrap()
}
fn origin(n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(91),
        timestamp: Timestamp::from_nanos(n),
    }
}
fn query(left: u64, right: u64, frequency: u64, offset: i128, span: u64) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time(0, 1), left, right, frequency, offset, span).unwrap()
}
fn owner(source: u32, target: u32) -> ConvertedMixer {
    let format = |hz| AudioFormat::new(hz, 1).unwrap();
    let bank = SampleBank::new(format(source), PcmLimits::new(1024, 1024, 1).unwrap()).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format(source),
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 64, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let mut owner = ConvertedMixer::new(
        mixer,
        format(target),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|f| panic!("fixture refused: {:?}", f.error()));
    owner.enable_phase_history(8).unwrap();
    owner
}
fn render(owner: &mut ConvertedMixer, n: usize) -> ConvertedRenderReport {
    owner.render(&mut [0.0; 16][..n]).unwrap()
}

#[test]
fn signed_extrapolation_uses_one_mathematical_floor() {
    for (left, right, hz, offset, host_span, expected) in [
        (10, 20, 3, -2, 7, 2_380_952_380),
        (10, 20, 3, 9, 7, 7_619_047_619),
        (1, 4, 10, -5, 3, -400_000_000),
        (0, 1, 3, -1, 2, -166_666_667),
    ] {
        let q = query(left, right, hz, offset, host_span);
        assert_eq!(q.point(origin(0)).unwrap(), origin(expected));
        assert_eq!(
            q.elapsed_nanos_since_source_frame(0, 1),
            if expected < 0 {
                Err(ConvertedPhaseError::NegativeElapsed)
            } else {
                Ok(i128::from(expected))
            }
        );
    }
}

#[test]
fn tiny_signed_grid_matches_independent_integer_rational_oracle() {
    // Common denominator expression is an independent small exact oracle.
    // No implementation interpolation or timestamp result participates in the expected value.
    for hz in 1..=5u64 {
        for h in 1..=5u64 {
            for left in 0..=3u64 {
                for delta in 0..=3u64 {
                    for offset in -7..=9i128 {
                        let denominator = 7 * i128::from(hz) * i128::from(h);
                        let numerator = i128::from(hz * h)
                            + 7 * i128::from(left * h)
                            + 7 * i128::from(delta) * offset;
                        let q = InterpolatedTargetTime::new(
                            time(1, 7),
                            left,
                            left + delta,
                            hz,
                            offset,
                            h,
                        )
                        .unwrap();
                        let nanos = (numerator * 1_000_000_000).div_euclid(denominator);
                        assert_eq!(
                            q.point(origin(-17)).unwrap().timestamp,
                            Timestamp::from_nanos((nanos - 17) as i64)
                        );
                        for cut in 0..=2u64 {
                            let elapsed_numerator = numerator * 3 - i128::from(cut) * denominator;
                            let expected = if elapsed_numerator < 0 {
                                Err(ConvertedPhaseError::NegativeElapsed)
                            } else {
                                Ok((elapsed_numerator * 1_000_000_000).div_euclid(denominator * 3))
                            };
                            assert_eq!(q.elapsed_nanos_since_source_frame(cut, 3), expected);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn huge_frequency_and_host_span_keep_subnanosecond_cancellation() {
    let f = u64::MAX;
    let h = u64::MAX - 1;
    let q = InterpolatedTargetTime::new(time(1, 1_000_000_000), f - 1, f, f, i128::from(h - 1), h)
        .unwrap();
    // Q = 1 + 1ns - 1/(F*H), with a denominator wider than TargetTime can store.
    assert_eq!(
        q.compare_target_time(TargetTime::new(1, 0, 1).unwrap()),
        Ordering::Greater
    );
    assert_eq!(
        q.compare_target_time(TargetTime::new(1, 1, 1_000_000_000).unwrap()),
        Ordering::Less
    );
    assert_eq!(
        q.point(origin(0)).unwrap().timestamp,
        Timestamp::from_nanos(1_000_000_000)
    );
    assert_eq!(q.elapsed_nanos_since_source_frame(1, 1), Ok(0));
}

#[test]
fn equal_ticks_zero_offsets_and_fractional_base_remain_exact() {
    let q = InterpolatedTargetTime::new(time(1, 3), 1, 1, 7, -i128::from(u64::MAX), 1).unwrap();
    assert_eq!(q.compare_target_time(time(10, 21)), Ordering::Equal);
    assert_eq!(
        q.point(origin(0)).unwrap().timestamp,
        Timestamp::from_nanos(476_190_476)
    );
    let at_left = InterpolatedTargetTime::new(time(1, 3), 1, u64::MAX, 7, 0, u64::MAX).unwrap();
    assert_eq!(at_left.compare_target_time(time(10, 21)), Ordering::Equal);
}

#[test]
fn constructor_refuses_invalid_or_unbounded_arguments_without_abs_min_panic() {
    for (left, right, f, offset, h, error) in [
        (0, 1, 0, 0, 1, AudioError::InvalidFormat),
        (0, 1, 1, 0, 0, AudioError::InvalidFormat),
        (2, 1, 1, 0, 1, AudioError::InvalidFormat),
        (0, 1, 1, i128::MIN, 1, AudioError::Overflow),
        (0, 1, 1, i128::from(u64::MAX) + 1, 1, AudioError::Overflow),
        (0, 1, 1, -i128::from(u64::MAX) - 1, 1, AudioError::Overflow),
        (
            0,
            u64::MAX,
            1,
            i128::from(u64::MAX),
            1,
            AudioError::Overflow,
        ),
    ] {
        assert!(
            matches!(InterpolatedTargetTime::new(time(0, 1), left, right, f, offset, h), Err(e) if e == error)
        );
    }
}

#[test]
fn constructor_cancels_unsigned_drift_before_signed_cast_including_min() {
    let base = TargetTime::new((1u64 << 63) - 1, 0, 1).unwrap();
    let q = InterpolatedTargetTime::new(base, 0, u64::MAX, 1, -((1i128 << 63) + 1), 1).unwrap();
    assert_eq!(q.compare_target_time(time(0, 1)), Ordering::Less);
    assert_eq!(q.point(origin(i64::MAX)), Err(AudioError::Overflow));
    assert_eq!(
        q.elapsed_nanos_since_source_frame(0, 1),
        Err(ConvertedPhaseError::Overflow)
    );
    assert!(matches!(
        InterpolatedTargetTime::new(time(0, 1), 0, u64::MAX, 1, -((1i128 << 63) + 1), 1),
        Err(AudioError::Overflow)
    ));
}

#[test]
fn clock_origin_is_added_before_i64_narrowing() {
    let positive = query(10_000_000_000, 10_000_000_000, 1, 0, 1);
    assert_eq!(
        positive.point(origin(i64::MIN)).unwrap().timestamp,
        Timestamp::from_nanos(776_627_963_145_224_192)
    );
    assert_eq!(positive.point(origin(0)), Err(AudioError::Overflow));
    let negative = query(0, 10_000_000_000, 1, -1, 1);
    assert_eq!(
        negative.point(origin(i64::MAX)).unwrap().timestamp,
        Timestamp::from_nanos(-776_627_963_145_224_193)
    );
    assert_eq!(negative.point(origin(0)), Err(AudioError::Overflow));
}

#[test]
fn elapsed_i128_max_and_cut_cancellation_do_not_overflow_early() {
    let base = TargetTime::new(15_767_830_579_797_665_575, 884_105_727, 1_000_000_000).unwrap();
    let q = InterpolatedTargetTime::new(base, 0, u64::MAX, 1, 9_223_372_036, 1).unwrap();
    assert_eq!(q.elapsed_nanos_since_source_frame(0, 1), Ok(i128::MAX));
    let plus = TargetTime::new(base.seconds(), 884_105_728, 1_000_000_000).unwrap();
    let q = InterpolatedTargetTime::new(plus, 0, u64::MAX, 1, 9_223_372_036, 1).unwrap();
    assert_eq!(
        q.elapsed_nanos_since_source_frame(0, 1),
        Err(ConvertedPhaseError::Overflow)
    );
    assert_eq!(
        q.elapsed_nanos_since_source_frame(1, 1),
        Ok(i128::MAX - 999_999_999)
    );
}

#[test]
fn representable_negative_i128_nanoseconds_are_negative_not_overflow() {
    let base = TargetTime::new(2_678_913_493_911_886_039, 115_894_272, 1_000_000_000).unwrap();
    let q = InterpolatedTargetTime::new(base, 0, u64::MAX, 1, -9_223_372_037, 1).unwrap();
    assert_eq!(
        q.elapsed_nanos_since_source_frame(0, 1),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn midpoint_keeps_source_fraction_instead_of_inverting_rounded_timestamp() {
    let q = query(2, 3, 44_100, 1, 2);
    assert_eq!(q.elapsed_nanos_since_source_frame(2, 44_100), Ok(11_337));
    assert_eq!(
        q.elapsed_nanos_since_source_frame(0, 0),
        Err(ConvertedPhaseError::InvalidDescriptor)
    );
    assert_eq!(
        q.elapsed_nanos_since_source_frame(3, 44_100),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn actual_active_spans_preserve_both_rate_directions() {
    for (source, target, frames, expected) in
        [(44_100, 48_000, 3, 17_148), (48_000, 44_100, 2, 3_684)]
    {
        let mut owner = owner(source, target);
        let end = render(&mut owner, frames).target_end_time;
        let span = owner.phase_history().unwrap().span(0).unwrap();
        assert_eq!(
            span.elapsed_nanos_at_interpolated_time(
                query(0, frames as u64, u64::from(target), 1, 1),
                end,
                2
            ),
            Ok(expected)
        );
    }
}

#[test]
fn actual_held_span_uses_anchor_not_interpolated_source_endpoints() {
    let mut owner = owner(44_100, 48_000);
    let start = render(&mut owner, 4).target_end_time;
    let end = owner.render_held(&mut [0.0; 3]).unwrap().target_end_time;
    let span = owner.phase_history().unwrap().span(1).unwrap();
    for offset in [0, 1, 2, 3, 6] {
        let q = InterpolatedTargetTime::new(start, 0, 3, 48_000, offset, 6).unwrap();
        assert_eq!(
            span.elapsed_nanos_at_interpolated_time(q, end, 3),
            Ok(15_306)
        );
    }
}

#[test]
fn actual_resume_and_retarget_keep_piecewise_physical_duration() {
    let mut owner = owner(1, 3);
    render(&mut owner, 1);
    owner
        .retarget(
            AudioFormat::new(5, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            16,
        )
        .unwrap();
    owner.render_held(&mut [0.0; 1]).unwrap();
    owner
        .retarget(
            AudioFormat::new(3, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            16,
        )
        .unwrap();
    let end = render(&mut owner, 1).target_end_time;
    let span = owner.phase_history().unwrap().span(2).unwrap();
    let q = InterpolatedTargetTime::new(time(8, 15), 0, 1, 3, 2, 5).unwrap();
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(q, end, 0),
        Ok(466_666_666)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(q, end, 1),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn exact_span_range_rejects_signed_before_and_after_without_native_claims() {
    let span = ConvertedPhaseSpan::from_words([3, 2, 0, 1, 0, 2, 3, 0]).unwrap();
    let start = time(2, 3);
    let end = TargetTime::new(1, 0, 1).unwrap();
    let make = |offset| InterpolatedTargetTime::new(start, 0, 1, 3, offset, 1).unwrap();
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(make(0), start, 2),
        Ok(0)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(make(1), end, 2),
        Ok(333_333_333)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(make(-1), end, 2),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(make(2), end, 2),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(make(0), time(1, 3), 2),
        Err(ConvertedPhaseError::InvalidRange)
    );
    // Caller-supplied qualified_end is a mathematical bound, never proof of a native write.
}

#[test]
fn highest_fraction_products_preserve_independent_fraction_literals() {
    let m = u64::MAX;
    let span =
        ConvertedPhaseSpan::from_words([u64::from(u32::MAX - 4), 3, 11, m - 154, 1, 7, m - 130, 0])
            .unwrap();
    let base = TargetTime::new(1, 5, m - 58).unwrap();
    let end = TargetTime::new(3, 0, 1).unwrap();
    // Python Fraction oracle denominator has 347 bits after reduction.
    for (offset, expected) in [
        (i128::from(m - 106) + 40, 1_000_000_000),
        (-40, 500_000_000),
    ] {
        let q = InterpolatedTargetTime::new(base, m / 2, m - 1, m - 82, offset, m - 106).unwrap();
        assert_eq!(
            span.elapsed_nanos_at_interpolated_time(q, end, 0),
            Ok(expected)
        );
    }
}

#[test]
fn actual_high_limb_span_resolves_signed_near_nanosecond_crossings() {
    let mut owner = owner(4_294_967_291, 4_294_967_279);
    render(&mut owner, 1);
    owner
        .retarget(
            AudioFormat::new(4_294_967_231, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            16,
        )
        .unwrap();
    render(&mut owner, 1);
    owner.render_held(&mut [0.0; 1]).unwrap();
    let end = render(&mut owner, 16).target_end_time;
    let span = owner.phase_history().unwrap().span(2).unwrap();
    assert_eq!(
        span.to_words(),
        [
            4_294_967_291,
            2,
            309_237_643_512,
            18_446_743_721_522_234_449,
            0,
            12_884_901_789,
            18_446_743_721_522_234_449,
            0
        ]
    );
    let base = time(12_884_901_789, 18_446_743_721_522_234_449);
    for (offset, expectations) in [
        (i128::from(u64::MAX), [0, 0, 1, 1, 1, 1, 1]),
        (-i128::from(u64::MAX), [0, 0, 0, 0, 0, 0, 1]),
    ] {
        for (index, expected) in expectations.into_iter().enumerate() {
            let left = 9_856_809_399 + index as u64 - 3;
            let q = InterpolatedTargetTime::new(
                base,
                left,
                left + 1,
                18_446_744_073_709_551_557,
                offset,
                9_223_372_036_854_775_815,
            )
            .unwrap();
            assert_eq!(
                span.elapsed_nanos_at_interpolated_time(q, end, 0),
                Ok(expected)
            );
            assert_eq!(span.elapsed_nanos_at_interpolated_time(q, end, 2), Ok(0));
        }
    }
}

#[test]
fn continuous_retarget_preserves_existing_point_and_new_exact_query_agreement() {
    let mut owner = owner(44_100, 48_000);
    let first_end = render(&mut owner, 4).target_end_time;
    owner
        .retarget(
            AudioFormat::new(44_100, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            16,
        )
        .unwrap();
    let end = render(&mut owner, 1).target_end_time;
    let history = owner.phase_history().unwrap();
    assert_eq!(history.len(), 1);
    let span = history.span(0).unwrap();
    let q = InterpolatedTargetTime::new(first_end, 0, 1, 44_100, 1, 1).unwrap();
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(q, end, 4),
        Ok(15_306)
    );
    assert_eq!(
        span.point(end, end).unwrap().elapsed_nanos_since(4),
        Ok(15_306)
    );
}

#[test]
fn constructor_and_wide_queries_allocate_nothing() {
    let m = u64::MAX;
    let span =
        ConvertedPhaseSpan::from_words([u64::from(u32::MAX - 4), 3, 11, m - 154, 1, 7, m - 130, 0])
            .unwrap();
    let base = TargetTime::new(1, 5, m - 58).unwrap();
    let end = TargetTime::new(3, 0, 1).unwrap();
    let (result, counts) = tracked(|| {
        let q = InterpolatedTargetTime::new(
            std::hint::black_box(base),
            m / 2,
            m - 1,
            m - 82,
            i128::from(m - 106) + 40,
            m - 106,
        )
        .unwrap();
        let order = q.compare_target_time(end);
        let elapsed = span.elapsed_nanos_at_interpolated_time(q, end, 0);
        (order, elapsed, q.point(origin(-11)))
    });
    assert_eq!(result.0, Ordering::Less);
    assert_eq!(result.1, Ok(1_000_000_000));
    assert!(result.2.is_ok());
    assert_eq!(counts, [0; 3]);
}
