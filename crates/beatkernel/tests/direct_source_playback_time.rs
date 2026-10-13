//! Direct source-time math never authenticates generation, submission, or pause ACK.
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
fn count(i: usize) {
    let _ = TRACK.try_with(|t| {
        if t.get() {
            let _ = COUNTS.try_with(|c| {
                let mut v = c.get();
                v[i] += 1;
                c.set(v);
            });
        }
    });
}
// SAFETY: Allocation arguments and pointers pass unchanged to System.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count(1);
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count(2);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn tracked<T>(f: impl FnOnce() -> T) -> (T, [usize; 3]) {
    COUNTS.with(|c| c.set([0; 3]));
    TRACK.with(|t| t.set(true));
    let r = f();
    TRACK.with(|t| t.set(false));
    (r, COUNTS.with(Cell::get))
}
fn time(n: u64, d: u64) -> TargetTime {
    TargetTime::new(n / d, n % d, d).unwrap()
}
fn q(t: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(t, 0, 0, 1, 0, 1).unwrap()
}
fn origin(n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(3),
        timestamp: Timestamp::from_nanos(n),
    }
}
fn span(
    start: u64,
    end: Option<u64>,
    playback: u64,
    state: SourcePlaybackState,
) -> SourcePlaybackSpan {
    SourcePlaybackSpan::new(start, end, playback, state).unwrap()
}
fn owner(source: u32, target: u32, gated: bool) -> (CommandProducer, ConvertedMixer) {
    let format = AudioFormat::new(source, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(4096, 4096, 1).unwrap()).unwrap();
    let (p, c) = if gated {
        command_queue_with_start_gate(8)
    } else {
        command_queue(8)
    }
    .unwrap();
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(8, 4, 8, 256, 8).unwrap(),
        ),
        bank,
        c,
    )
    .unwrap();
    let mut o = ConvertedMixer::new(
        mixer,
        AudioFormat::new(target, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|f| panic!("fixture refused: {:?}", f.error()));
    o.enable_source_playback_history(8).unwrap();
    (p, o)
}
fn render(o: &mut ConvertedMixer, n: usize) -> ConvertedRenderReport {
    o.render(&mut [0.0; 32][..n]).unwrap()
}

#[test]
fn source_midpoint_is_floored_only_after_subtracting_cut() {
    let running = span(0, None, 0, SourcePlaybackState::Running);
    let midpoint = q(time(5, 88_200));
    assert_eq!(
        midpoint.compare_source_time_frame(2, 44_100),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        midpoint.compare_source_time_frame(3, 44_100),
        Ok(Ordering::Less)
    );
    assert_eq!(
        running.elapsed_nanos_at_source_time(midpoint, 44_100, 2),
        Ok(11_337)
    );
    // Separately rounded absolute values give the wrong 11_338 ns.
    assert_eq!(
        midpoint.point(origin(0)).unwrap().timestamp.as_nanos() - 45_351,
        11_338
    );
    assert_eq!(
        running.elapsed_nanos_at_source_time(q(time(3, 44_100)), 44_100, 2),
        Ok(22_675)
    );
}

#[test]
fn frozen_control_and_fractional_resume_remain_distinct() {
    let frozen = span(3, Some(128), 3, SourcePlaybackState::Frozen);
    let resumed = span(128, None, 3, SourcePlaybackState::Running);
    assert_eq!(
        frozen.elapsed_nanos_at_source_time(q(time(127, 44_100)), 44_100, 2),
        Ok(22_675)
    );
    assert_eq!(
        resumed.elapsed_nanos_at_source_time(q(time(128, 44_100)), 44_100, 2),
        Ok(22_675)
    );
    assert_eq!(
        resumed.elapsed_nanos_at_source_time(q(time(1029, 352_800)), 44_100, 2),
        Ok(36_848)
    );
}

#[test]
fn source_bounds_are_exact_half_open_before_duration_floor() {
    let running = span(2, Some(3), 1, SourcePlaybackState::Running);
    for (offset, expected) in [
        (-1, Err(ConvertedPhaseError::BeforeRetainedRange)),
        (0, Ok(22_675)),
        (1, Ok(22_675)),
    ] {
        let query =
            InterpolatedTargetTime::new(time(2, 44_100), 0, 1, u64::MAX, offset, u64::MAX - 1)
                .unwrap();
        assert_eq!(
            running.elapsed_nanos_at_source_time(query, 44_100, 0),
            expected
        );
    }
    assert_eq!(
        running.elapsed_nanos_at_source_time(q(time(3, 44_100)), 44_100, 0),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
}

#[test]
fn zero_rate_negative_query_and_negative_elapsed_refuse() {
    let running = span(0, None, 0, SourcePlaybackState::Running);
    assert_eq!(
        q(time(0, 1)).compare_source_time_frame(0, 0),
        Err(ConvertedPhaseError::InvalidDescriptor)
    );
    assert_eq!(
        running.elapsed_nanos_at_source_time(q(time(0, 1)), 0, 0),
        Err(ConvertedPhaseError::InvalidDescriptor)
    );
    let negative = InterpolatedTargetTime::new(time(0, 1), 0, 1, 1, -1, 1).unwrap();
    assert_eq!(
        running.elapsed_nanos_at_source_time(negative, 1, 0),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    );
    assert_eq!(
        running.elapsed_nanos_at_source_time(q(time(0, 1)), 1, 1),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
    assert_eq!(
        span(2, None, 1, SourcePlaybackState::Frozen).elapsed_nanos_at_source_time(
            q(time(2, 1)),
            1,
            2
        ),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn effective_cut_above_u64_and_large_frozen_differences_are_supported() {
    let running = span(u64::MAX, None, 0, SourcePlaybackState::Running);
    let next = InterpolatedTargetTime::new(time(u64::MAX, 1), 1, 1, 1, 0, 1).unwrap();
    assert_eq!(running.elapsed_nanos_at_source_time(next, 1, 1), Ok(0));
    let midpoint = InterpolatedTargetTime::new(time(u64::MAX, 1), 3, 3, 2, 0, 1).unwrap();
    assert_eq!(
        running.elapsed_nanos_at_source_time(midpoint, 1, 1),
        Ok(500_000_000)
    );
    let frozen = span(u64::MAX, None, u64::MAX, SourcePlaybackState::Frozen);
    assert_eq!(
        frozen.elapsed_nanos_at_source_time(next, 1, u64::MAX - 1),
        Ok(1_000_000_000)
    );
    assert_eq!(
        frozen.elapsed_nanos_at_source_time(next, 1, 0),
        Ok(i128::from(u64::MAX) * 1_000_000_000)
    );
}

#[test]
fn actual_source_history_retains_historical_pause_resume_without_converter_descriptor() {
    let (mut p, mut o) = owner(1000, 1000, false);
    render(&mut o, 4);
    p.request_pause(true);
    render(&mut o, 4);
    render(&mut o, 4);
    p.request_pause(false);
    render(&mut o, 4);
    let h = o.source_playback_history().unwrap();
    for (physical, playback, state) in [
        (4, 4, SourcePlaybackState::Running),
        (5, 5, SourcePlaybackState::Frozen),
        (12, 5, SourcePlaybackState::Frozen),
        (13, 5, SourcePlaybackState::Running),
        (16, 8, SourcePlaybackState::Running),
    ] {
        let query = q(time(physical, 1000));
        let selected = h.lookup_at_source_time(query).unwrap();
        assert_eq!(selected.state(), state);
        assert_eq!(
            selected.elapsed_nanos_at_source_time(query, 1000, 0),
            Ok(playback * 1_000_000)
        );
    }
}

#[test]
fn actual_start_gate_and_retarget_preserve_direct_history() {
    let (mut p, mut o) = owner(1000, 1000, true);
    p.schedule_start_at(2).unwrap();
    render(&mut o, 4);
    assert_eq!(p.applied_start_frame(), Some(2));
    o.retarget(
        AudioFormat::new(48_000, 1).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        32,
    )
    .unwrap();
    render(&mut o, 8);
    let h = o.source_playback_history().unwrap();
    assert_eq!(
        h.lookup_at_source_time(q(time(1, 1000))).unwrap().state(),
        SourcePlaybackState::Frozen
    );
    let running = h.lookup_at_source_time(q(time(2, 1000))).unwrap();
    assert_eq!(running.state(), SourcePlaybackState::Running);
    assert_eq!(
        running.elapsed_nanos_at_source_time(q(time(3, 1000)), 1000, 0),
        Ok(1_000_000)
    );
}

#[test]
fn empty_history_and_fractional_future_refuse_but_pulled_endpoint_is_inclusive() {
    let (_, mut o) = owner(44_100, 8000, false);
    assert_eq!(
        o.source_playback_history()
            .unwrap()
            .lookup_at_source_time(q(time(0, 1))),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
    let report = render(&mut o, 1);
    assert_eq!(report.pulled_source_frame_cursor, 5);
    let h = o.source_playback_history().unwrap();
    assert!(h.lookup_at_source_time(q(time(5, 44_100))).is_ok());
    let future =
        InterpolatedTargetTime::new(time(5, 44_100), 0, 1, u64::MAX, 1, u64::MAX - 1).unwrap();
    assert_eq!(
        h.lookup_at_source_time(future),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
    let negative = InterpolatedTargetTime::new(time(0, 1), 0, 1, 1, -1, 1).unwrap();
    assert_eq!(
        h.lookup_at_source_time(negative),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    );
}

#[test]
fn actual_history_selects_new_interval_at_subnanosecond_transition() {
    let (mut p, mut o) = owner(44_100, 48_000, false);
    let first = render(&mut o, 4);
    p.request_pause(true);
    render(&mut o, 4);
    for (offset, state) in [
        (-1, SourcePlaybackState::Running),
        (0, SourcePlaybackState::Frozen),
        (1, SourcePlaybackState::Frozen),
    ] {
        let query = InterpolatedTargetTime::new(
            time(first.pulled_source_frame_cursor, 44_100),
            0,
            1,
            u64::MAX,
            offset,
            u64::MAX - 1,
        )
        .unwrap();
        assert_eq!(
            o.source_playback_history()
                .unwrap()
                .lookup_at_source_time(query)
                .unwrap()
                .state(),
            state
        );
    }
}

#[test]
fn native_counter_progress_distinguishes_identical_floored_clock_points() {
    let one = q(time(1, 4_000_000_000));
    let two = q(time(2, 4_000_000_000));
    assert_eq!(one.point(origin(0)), two.point(origin(0)));
    assert_eq!(
        one.compare_native_time_with_offset(time(0, 1), 2, 4_000_000_000, 0),
        Ok(Ordering::Less)
    );
    assert_eq!(
        two.compare_native_time_with_offset(time(0, 1), 1, 4_000_000_000, 0),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        two.compare_native_time_with_offset(time(0, 1), 2, 4_000_000_000, 0),
        Ok(Ordering::Equal)
    );
}

#[test]
fn exact_ahead_limit_uses_fraction_even_when_all_points_floor_identically() {
    // Native time 1/4 ns, maximum ahead 1 ns: exact limit 1.25 ns.
    for (offset, order) in [
        (-1, Ordering::Less),
        (0, Ordering::Equal),
        (1, Ordering::Greater),
    ] {
        let query = InterpolatedTargetTime::new(
            time(1, 1_000_000_000),
            1,
            2,
            4_000_000_000,
            offset,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(query.point(origin(0)).unwrap(), origin(1));
        assert_eq!(
            query.compare_native_time_with_offset(time(0, 1), 1, 4_000_000_000, 1),
            Ok(order)
        );
    }
}

#[test]
fn huge_host_denominator_cancellation_is_compared_without_narrowing() {
    let f = u64::MAX;
    let h = f - 1;
    let query =
        InterpolatedTargetTime::new(time(1, 1_000_000_000), f - 1, f, f, i128::from(h - 1), h)
            .unwrap();
    // Q = 1 + 1 ns - 1/(F H); the comparator's exact bound is 1 + 1 ns.
    assert_eq!(
        query.compare_native_time_with_offset(time(1, 1_000_000_000), f, f, 0),
        Ok(Ordering::Less)
    );
    assert_eq!(
        query.compare_native_time_with_offset(time(0, 1), f, f, 1),
        Ok(Ordering::Less)
    );
    assert_eq!(
        query.compare_native_time_with_offset(time(0, 1), f, f, 0),
        Ok(Ordering::Greater)
    );
}

#[test]
fn signed_nanosecond_extremes_and_non_nanosecond_base_compare_exactly() {
    for offset in [i64::MIN, -1, 0, 1, i64::MAX] {
        let query =
            InterpolatedTargetTime::new(time(1, 3), 0, 1, 1_000_000_000, i128::from(offset), 1)
                .unwrap();
        assert_eq!(
            query.compare_native_time_with_offset(time(1, 3), 0, 7, offset),
            Ok(Ordering::Equal)
        );
    }
    assert_eq!(
        q(time(0, 1)).compare_native_time_with_offset(time(0, 1), 0, 1, -1),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        q(time(0, 1)).compare_native_time_with_offset(time(0, 1), 0, 0, 0),
        Err(AudioError::InvalidFormat)
    );
}

#[test]
fn i128_min_whole_uses_coarse_comparison_without_subtraction_overflow() {
    let query = InterpolatedTargetTime::new(
        time((1u64 << 63) - 1, 1),
        0,
        u64::MAX,
        1,
        -((1i128 << 63) + 1),
        1,
    )
    .unwrap();
    assert_eq!(
        query.compare_native_time_with_offset(time(u64::MAX, 1), u64::MAX, 1, i64::MAX),
        Ok(Ordering::Less)
    );
    assert_eq!(
        query.compare_native_time_with_offset(time(0, 1), 0, 1, i64::MIN),
        Ok(Ordering::Less)
    );
    assert_eq!(
        query.elapsed_nanos_since_source_frame(0, 1),
        Err(ConvertedPhaseError::Overflow)
    );
}

#[test]
fn direct_queries_and_native_comparisons_allocate_nothing_after_setup() {
    let (_, mut o) = owner(44_100, 48_000, false);
    render(&mut o, 8);
    let h = o.source_playback_history().unwrap();
    let query = q(time(5, 88_200));
    let (result, counts) = tracked(|| {
        let selected = h.lookup_at_source_time(query).unwrap();
        (
            selected.elapsed_nanos_at_source_time(query, 44_100, 2),
            query.compare_source_time_frame(2, 44_100),
            query.compare_native_time_with_offset(time(1, 3), u64::MAX, u64::MAX, -1),
            h.lookup_at_source_time(q(time(1, 1))),
        )
    });
    assert_eq!(counts, [0; 3]);
    assert_eq!(result.0, Ok(11_337));
    assert_eq!(result.1, Ok(Ordering::Greater));
    assert_eq!(result.2, Ok(Ordering::Less));
    assert_eq!(result.3, Err(ConvertedPhaseError::AfterGeneratedRange));
}

#[test]
fn six_nonzero_fraction_terms_cover_the_414_bit_denominator_product() {
    // Independent rational arithmetic: the two queries straddle the exact bound
    // while all three absolute durations floor to 1_000_000_001 ns.
    // These constructible coordinates authenticate no native observation.
    let m = u64::MAX;
    let f = m - 58;
    let h = m - 82;
    let counter_frequency = m - 118;
    let bound_base = time(2, m - 105);
    for (numerator, expected) in [
        (18_446_744_074, Ordering::Less),
        (18_446_744_075, Ordering::Greater),
    ] {
        let query =
            InterpolatedTargetTime::new(time(numerator, m - 94), f - 1, f, f, i128::from(h - 1), h)
                .unwrap();
        assert_eq!(query.point(origin(0)).unwrap(), origin(1_000_000_001));
        let bound = InterpolatedTargetTime::new(
            bound_base,
            counter_frequency - 1,
            counter_frequency - 1,
            counter_frequency,
            0,
            1,
        )
        .unwrap();
        assert_eq!(bound.point(origin(1)).unwrap(), origin(1_000_000_001));
        let (result, counts) = tracked(|| {
            query.compare_native_time_with_offset(
                bound_base,
                counter_frequency - 1,
                counter_frequency,
                1,
            )
        });
        assert_eq!(result, Ok(expected));
        assert_eq!(counts, [0; 3]);
    }
}
