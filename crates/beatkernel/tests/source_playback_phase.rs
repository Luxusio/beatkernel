//! Caller-qualified source intervals are mathematical descriptors, never native pause ACKs.
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
// SAFETY: Allocation arguments and pointers pass unchanged to System.
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
    TargetTime::new(n / d, n % d, d).unwrap()
}
fn q(time: TargetTime) -> InterpolatedTargetTime {
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).unwrap()
}
fn interval(
    start: u64,
    end: Option<u64>,
    playback: u64,
    state: SourcePlaybackState,
) -> SourcePlaybackSpan {
    SourcePlaybackSpan::new(start, end, playback, state).unwrap()
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
// A transferred structural descriptor does not prove any generated or native interval.
fn math_span(
    hz: u32,
    source: u64,
    numerator: u64,
    denominator: u64,
    state: u64,
) -> ConvertedPhaseSpan {
    ConvertedPhaseSpan::from_words([
        u64::from(hz),
        source,
        numerator,
        denominator,
        0,
        0,
        1,
        state,
    ])
    .unwrap()
}

#[test]
fn descriptors_retain_exact_fields_and_reject_empty_reversed_or_impossible_playback() {
    for state in [SourcePlaybackState::Running, SourcePlaybackState::Frozen] {
        let span = interval(3, Some(128), 3, state);
        assert_eq!(span.physical_start(), 3);
        assert_eq!(span.physical_end(), Some(128));
        assert_eq!(span.playback_start(), 3);
        assert_eq!(span.state(), state);
        assert_eq!(span, span);
        assert!(!format!("{span:?}").is_empty());
        for (start, end, playback) in [
            (3, Some(3), 3),
            (3, Some(2), 3),
            (3, None, 4),
            (0, Some(1), 1),
        ] {
            assert_eq!(
                SourcePlaybackSpan::new(start, end, playback, state),
                Err(ConvertedPhaseError::InvalidDescriptor)
            );
        }
        assert!(SourcePlaybackSpan::new(u64::MAX, None, u64::MAX, state).is_ok());
    }
}

#[test]
fn actual_converter_fraction_clamps_caller_qualified_frozen_playback() {
    let mut owner = owner(44_100, 48_000);
    let end = render(&mut owner, 4).target_end_time;
    let span = owner.phase_history().unwrap().span(0).unwrap();
    let frozen = interval(3, Some(128), 3, SourcePlaybackState::Frozen);
    assert_eq!(
        span.compare_source_frame_at_interpolated_time(q(end), end, 3),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(q(end), end, 2),
        Ok(37_981)
    );
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(end), end, frozen, 2),
        Ok(22_675)
    );
    // Target 3.5 maps to physical 1029/320 source frames. This is pre-ACK math only.
    let midpoint = InterpolatedTargetTime::new(time(0, 1), 3, 4, 48_000, 1, 2).unwrap();
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(midpoint, end, frozen, 2),
        Ok(22_675)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(midpoint, end, 2),
        Ok(27_565)
    );
}

#[test]
fn source_selection_preserves_tiny_before_equal_after_despite_identical_nanoseconds() {
    let mut owner = owner(44_100, 48_000);
    let end = render(&mut owner, 4).target_end_time;
    let span = owner.phase_history().unwrap().span(0).unwrap();
    let running = interval(0, Some(3), 0, SourcePlaybackState::Running);
    let frozen = interval(3, None, 3, SourcePlaybackState::Frozen);
    let origin = ClockPoint {
        domain: ClockDomainId(3),
        timestamp: Timestamp::ZERO,
    };
    for (offset, order) in [
        (-1, Ordering::Less),
        (0, Ordering::Equal),
        (1, Ordering::Greater),
    ] {
        let query =
            InterpolatedTargetTime::new(time(3, 44_100), 0, 1, u64::MAX, offset, u64::MAX - 1)
                .unwrap();
        assert_eq!(
            query.point(origin).unwrap().timestamp,
            Timestamp::from_nanos(68_027)
        );
        assert_eq!(
            span.compare_source_frame_at_interpolated_time(query, end, 3),
            Ok(order)
        );
        if offset < 0 {
            assert_eq!(
                span.playback_elapsed_nanos_at_interpolated_time(query, end, running, 2),
                Ok(22_675)
            );
            assert_eq!(
                span.playback_elapsed_nanos_at_interpolated_time(query, end, frozen, 2),
                Err(ConvertedPhaseError::BeforeRetainedRange)
            );
        } else {
            assert_eq!(
                span.playback_elapsed_nanos_at_interpolated_time(query, end, running, 2),
                Err(ConvertedPhaseError::AfterGeneratedRange)
            );
            assert_eq!(
                span.playback_elapsed_nanos_at_interpolated_time(query, end, frozen, 2),
                Ok(22_675)
            );
        }
    }
}

#[test]
fn resume_fraction_keeps_actual_presentation_distinct_from_frozen_control() {
    let span = math_span(44_100, 128, 5, 8, 0);
    let resume = interval(128, None, 3, SourcePlaybackState::Running);
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(0, 1)), time(0, 1), resume, 2),
        Ok(36_848)
    );
    let control = math_span(44_100, 128, 0, 1, 0);
    assert_eq!(
        control.playback_elapsed_nanos_at_interpolated_time(q(time(0, 1)), time(0, 1), resume, 2),
        Ok(22_675)
    );
    // Both are mathematical descriptors; neither grants NativePause ACK.
}

#[test]
fn actual_converter_held_state_does_not_synthesize_playback_frozen_state() {
    let mut owner = owner(44_100, 48_000);
    render(&mut owner, 4);
    let end = owner.render_held(&mut [0.0; 3]).unwrap().target_end_time;
    let held = owner.phase_history().unwrap().span(1).unwrap();
    let running = interval(0, None, 0, SourcePlaybackState::Running);
    let frozen = interval(3, None, 3, SourcePlaybackState::Frozen);
    assert_eq!(
        held.compare_source_frame_at_interpolated_time(q(end), end, 3),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        held.playback_elapsed_nanos_at_interpolated_time(q(end), end, running, 2),
        Ok(37_981)
    );
    assert_eq!(
        held.playback_elapsed_nanos_at_interpolated_time(q(end), end, frozen, 2),
        Ok(22_675)
    );
}

#[test]
fn repeated_pause_resume_intervals_remove_only_each_declared_source_gap() {
    let span = math_span(3, 0, 0, 1, 0);
    let end = TargetTime::new(10, 0, 1).unwrap();
    let cases = [
        (
            time(1, 2),
            interval(0, Some(3), 0, SourcePlaybackState::Running),
            500_000_000,
        ),
        (
            time(4, 3),
            interval(3, Some(7), 3, SourcePlaybackState::Frozen),
            1_000_000_000,
        ),
        (
            time(5, 2),
            interval(7, Some(9), 3, SourcePlaybackState::Running),
            1_166_666_666,
        ),
        (
            TargetTime::new(4, 0, 1).unwrap(),
            interval(9, Some(15), 5, SourcePlaybackState::Frozen),
            1_666_666_666,
        ),
        (
            TargetTime::new(5, 1, 6).unwrap(),
            interval(15, None, 5, SourcePlaybackState::Running),
            1_833_333_333,
        ),
    ];
    for (query, playback, expected) in cases {
        assert_eq!(
            span.playback_elapsed_nanos_at_interpolated_time(q(query), end, playback, 0),
            Ok(expected)
        );
    }
}

#[test]
fn startup_and_nonzero_playback_cut_share_one_relative_floor() {
    for hz in [3, 44_100] {
        let span = math_span(hz, 0, 0, 1, 0);
        let query = q(time(3, u64::from(hz)));
        let playback = interval(2, None, 2, SourcePlaybackState::Running);
        assert_eq!(
            span.playback_elapsed_nanos_at_interpolated_time(
                query,
                time(4, u64::from(hz)),
                playback,
                2
            ),
            Ok(if hz == 3 { 333_333_333 } else { 22_675 })
        );
    }
}

#[test]
fn independent_small_integer_grid_covers_running_and_frozen_mapping() {
    for hz in 1..=5u32 {
        let span = math_span(hz, 0, 0, 1, 0);
        let end = TargetTime::new(20, 0, 1).unwrap();
        for start in 0..=5u64 {
            for playback in 0..=start {
                for half in 0..=5u64 {
                    for cut in 0..=6u64 {
                        let physical_twice = 2 * start + half;
                        let query = q(time(physical_twice, 2 * u64::from(hz)));
                        for state in [SourcePlaybackState::Running, SourcePlaybackState::Frozen] {
                            let active_twice = 2 * playback
                                + if state == SourcePlaybackState::Running {
                                    half
                                } else {
                                    0
                                };
                            let relative = i128::from(active_twice) - 2 * i128::from(cut);
                            let expected = if relative < 0 {
                                Err(ConvertedPhaseError::NegativeElapsed)
                            } else {
                                Ok(relative * 1_000_000_000 / (2 * i128::from(hz)))
                            };
                            assert_eq!(
                                span.playback_elapsed_nanos_at_interpolated_time(
                                    query,
                                    end,
                                    interval(start, None, playback, state),
                                    cut
                                ),
                                expected
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn target_range_checks_precede_source_mapping_for_both_new_apis() {
    let span = ConvertedPhaseSpan::from_words([3, 2, 0, 1, 0, 2, 3, 0]).unwrap();
    let end = TargetTime::new(1, 0, 1).unwrap();
    let playback = interval(0, None, 0, SourcePlaybackState::Running);
    for (query, bound, error) in [
        (time(1, 3), end, ConvertedPhaseError::BeforeRetainedRange),
        (
            TargetTime::new(1, 1, u64::MAX).unwrap(),
            end,
            ConvertedPhaseError::AfterGeneratedRange,
        ),
        (time(2, 3), time(1, 3), ConvertedPhaseError::InvalidRange),
    ] {
        assert_eq!(
            span.compare_source_frame_at_interpolated_time(q(query), bound, 0),
            Err(error)
        );
        assert_eq!(
            span.playback_elapsed_nanos_at_interpolated_time(q(query), bound, playback, 0),
            Err(error)
        );
    }
    assert_eq!(
        span.compare_source_frame_at_interpolated_time(q(time(2, 3)), time(2, 3), 2),
        Ok(Ordering::Equal)
    );
}

#[test]
fn exclusive_source_end_and_negative_playback_elapsed_are_explicit() {
    let span = math_span(3, 2, 0, 1, 0);
    let playback = interval(2, Some(3), 1, SourcePlaybackState::Running);
    let end = time(1, 1);
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(1, 3)), end, playback, 0),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    );
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(0, 1)), end, playback, 2),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
    let frozen = interval(2, None, 1, SourcePlaybackState::Frozen);
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(0, 1)), end, frozen, 2),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn running_effective_cut_above_u64_is_not_prematurely_narrowed() {
    let span = math_span(1, u64::MAX, 0, 1, 0);
    let playback = interval(u64::MAX, None, 0, SourcePlaybackState::Running);
    let end = time(2, 1);
    assert_eq!(
        span.compare_source_frame_at_interpolated_time(q(time(1, 1)), end, u64::MAX),
        Ok(Ordering::Greater)
    );
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(1, 1)), end, playback, 1),
        Ok(0)
    );
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(3, 2)), end, playback, 1),
        Ok(500_000_000)
    );
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(1, 2)), end, playback, 1),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn u64_playback_difference_is_scaled_after_subtraction() {
    let span = math_span(1, u64::MAX, 0, 1, 1);
    let playback = interval(u64::MAX, None, u64::MAX, SourcePlaybackState::Frozen);
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(
            q(time(0, 1)),
            time(0, 1),
            playback,
            u64::MAX - 1
        ),
        Ok(1_000_000_000)
    );
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(q(time(0, 1)), time(0, 1), playback, 0),
        Ok(i128::from(u64::MAX) * 1_000_000_000)
    );
}

#[test]
fn actual_retarget_keeps_playback_mapping_and_legacy_physical_elapsed() {
    let mut owner = owner(44_100, 48_000);
    let first = render(&mut owner, 4).target_end_time;
    owner
        .retarget(
            AudioFormat::new(44_100, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            16,
        )
        .unwrap();
    let end = render(&mut owner, 1).target_end_time;
    let span = owner.phase_history().unwrap().span(0).unwrap();
    let query = InterpolatedTargetTime::new(first, 0, 1, 44_100, 1, 1).unwrap();
    let running = interval(3, None, 2, SourcePlaybackState::Running);
    assert_eq!(
        span.playback_elapsed_nanos_at_interpolated_time(query, end, running, 2),
        Ok(37_981)
    );
    assert_eq!(
        span.elapsed_nanos_at_interpolated_time(query, end, 4),
        Ok(15_306)
    );
    assert_eq!(
        span.point(end, end).unwrap().elapsed_nanos_since(4),
        Ok(15_306)
    );
}

#[test]
fn wide_anchor_and_signed_host_terms_compare_before_frozen_duration() {
    // Structural mathematical transfer only: no generated/native provenance is asserted.
    let m = u64::MAX;
    let span =
        ConvertedPhaseSpan::from_words([u64::from(u32::MAX - 4), 3, 11, m - 154, 1, 7, m - 130, 0])
            .unwrap();
    let base = TargetTime::new(1, 5, m - 58).unwrap();
    let end = time(3, 1);
    let frozen = interval(3, Some(m), 3, SourcePlaybackState::Frozen);
    for offset in [-40, i128::from(m - 106) + 40] {
        let query =
            InterpolatedTargetTime::new(base, m / 2, m - 1, m - 82, offset, m - 106).unwrap();
        assert_eq!(
            span.compare_source_frame_at_interpolated_time(query, end, 3),
            Ok(Ordering::Greater)
        );
        assert_eq!(
            span.compare_source_frame_at_interpolated_time(query, end, m),
            Ok(Ordering::Less)
        );
        assert_eq!(
            span.playback_elapsed_nanos_at_interpolated_time(query, end, frozen, 2),
            Ok(0)
        );
    }
}

#[test]
fn hot_mapping_comparison_and_refusal_allocate_nothing() {
    let span = math_span(44_100, 0, 0, 1, 0);
    let end = time(4, 44_100);
    let running = interval(0, Some(3), 0, SourcePlaybackState::Running);
    let frozen = interval(3, None, 3, SourcePlaybackState::Frozen);
    let (results, counts) = tracked(|| {
        let query =
            InterpolatedTargetTime::new(time(3, 44_100), 0, 1, u64::MAX, 1, u64::MAX - 1).unwrap();
        (
            span.compare_source_frame_at_interpolated_time(query, end, 3),
            span.playback_elapsed_nanos_at_interpolated_time(query, end, frozen, 2),
            span.playback_elapsed_nanos_at_interpolated_time(query, end, running, 2),
            SourcePlaybackSpan::new(3, Some(3), 3, SourcePlaybackState::Running),
        )
    });
    assert_eq!(results.0, Ok(Ordering::Greater));
    assert_eq!(results.1, Ok(22_675));
    assert_eq!(results.2, Err(ConvertedPhaseError::AfterGeneratedRange));
    assert_eq!(results.3, Err(ConvertedPhaseError::InvalidDescriptor));
    assert_eq!(counts, [0; 3]);
}
