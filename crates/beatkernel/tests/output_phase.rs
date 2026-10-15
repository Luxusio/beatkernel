//! Exact physical frame arithmetic. These builders do not establish native admission.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::{OutputPhaseBasis, OutputPhasePoint, Rate, Transport},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! { static TRACK: Cell<bool> = const { Cell::new(false) }; static ALLOCS: Cell<usize> = const { Cell::new(0) }; }
struct Allocator;
// SAFETY: All allocation operations forward the original arguments to System;
// instrumentation only accesses allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|t| {
            if t.get() {
                ALLOCS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|t| {
            if t.get() {
                ALLOCS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn clock(n: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(1),
        timestamp: ts(n),
    }
}
fn source(frame: u64, numerator: u64, denominator: u64) -> SourcePosition {
    SourcePosition {
        frame,
        numerator,
        denominator,
    }
}
fn basis(hz: u32, cut: u64) -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(1), 17, 3, cut, hz).unwrap()
}
fn p(b: OutputPhaseBasis, logical: i64, frame: u64) -> OutputPhasePoint {
    b.point(clock(logical), source(frame, 0, 1)).unwrap()
}
fn transport(
    b: OutputPhaseBasis,
    start: i64,
    preroll: i64,
    rate: Rate,
    end: Option<i64>,
) -> Transport {
    Transport::new_with_output_phase(
        b,
        clock(100),
        ts(start),
        Duration::from_nanos(preroll),
        rate,
        end.map(ts),
    )
    .unwrap()
}

#[test]
fn immutable_basis_and_point_preserve_original_clock_and_identity() {
    let b = basis(44_100, 2);
    assert_eq!(
        (
            b.domain(),
            b.owner(),
            b.generation(),
            b.active_origin_frame(),
            b.source_hz()
        ),
        (ClockDomainId(1), 17, 3, 2, 44_100)
    );
    let q = p(b, 68_027, 3);
    assert_eq!(q.basis(), b);
    assert_eq!(q.logical(), clock(68_027));
    assert_eq!(q.elapsed_nanos(), 22_675);
    assert!(OutputPhaseBasis::new(ClockDomainId(1), 0, 0, 0, 1).is_ok());
    assert!(OutputPhaseBasis::new(ClockDomainId(1), 17, 3, 2, 0).is_err());
}

#[test]
fn relative_floor_differs_from_subtraction_of_absolute_floors() {
    let b = basis(44_100, 2);
    assert_eq!(p(b, 68_027, 3).elapsed_nanos(), 22_675);
    assert_ne!(p(b, 68_027, 3).elapsed_nanos(), 68_027 - 45_351);
    let b = basis(3, 2);
    let t = transport(b, 0, 0, Rate::new(3, 2).unwrap(), None);
    assert_eq!(p(b, 1_000_000_000, 3).elapsed_nanos(), 333_333_333);
    assert_eq!(
        t.position_at_output_phase(p(b, 1_000_000_000, 3)).unwrap(),
        ts(499_999_999)
    );
}

#[test]
fn actual_converted_positions_keep_fraction_after_original_integer_cut() {
    for (hz, target, frames, expected_position, elapsed) in [
        (44_100, 48_000, 3, source(2, 121, 160), 17_148),
        (48_000, 44_100, 2, source(2, 26, 147), 3_684),
    ] {
        let format = AudioFormat::new(hz, 1).unwrap();
        let limits = AudioLimits::new(8, 1, 8, 16, 8).unwrap();
        let bank = SampleBank::new(format, PcmLimits::new(32, 32, 1).unwrap()).unwrap();
        let (_producer, consumer) = command_queue(8).unwrap();
        let mixer = Mixer::new(
            MixerConfig::new(format, ClockDomainId(1), Timestamp::ZERO, limits),
            bank,
            consumer,
        )
        .unwrap();
        let mut converted = ConvertedMixer::new(
            mixer,
            AudioFormat::new(target, 1).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            8,
        )
        .unwrap();
        let mut output = vec![99.0; frames];
        let report = converted.render(&mut output).unwrap();
        assert_eq!(report.source_position, expected_position);
        assert_eq!(output, vec![0.0; frames]);
        let q = basis(hz, 2)
            .point(clock(100), report.source_position)
            .unwrap();
        assert_eq!(q.elapsed_nanos(), elapsed);
        assert_eq!(q.logical(), clock(100));
        // Generated positions are genuine conversion evidence, not presentation ACKs.
        assert_eq!(report.state, ConvertedOutputState::Active);
    }
}

#[test]
fn fraction_and_whole_remainder_are_combined_before_final_floor() {
    let b = basis(3, 2);
    assert_eq!(
        b.point(clock(100), source(3, 2, 3))
            .unwrap()
            .elapsed_nanos(),
        555_555_555
    );
    assert_eq!(
        basis(44_100, 2)
            .point(clock(100), source(2, 121, 160))
            .unwrap()
            .elapsed_nanos(),
        17_148
    );
    assert_eq!(
        basis(48_000, 2)
            .point(clock(100), source(2, 26, 147))
            .unwrap()
            .elapsed_nanos(),
        3_684
    );
}

#[test]
fn largest_valid_fraction_and_full_unsigned_frame_span_are_representable() {
    let b = basis(1, 0);
    assert_eq!(
        b.point(clock(100), source(0, u64::MAX - 1, u64::MAX))
            .unwrap()
            .elapsed_nanos(),
        999_999_999
    );
    assert_eq!(
        b.point(clock(100), source(u64::MAX, 0, 1))
            .unwrap()
            .elapsed_nanos(),
        18_446_744_073_709_551_615_000_000_000i128
    );
}

#[test]
fn malformed_fraction_wrong_domain_and_before_cut_are_rejected() {
    let b = basis(44_100, 2);
    for s in [
        source(2, 0, 0),
        source(2, 1, 1),
        source(2, 4, 3),
        source(1, 1, 2),
    ] {
        assert!(b.point(clock(100), s).is_err());
    }
    assert!(b
        .point(
            ClockPoint {
                domain: ClockDomainId(9),
                timestamp: ts(100)
            },
            source(2, 0, 1)
        )
        .is_err());
}

#[test]
fn four_selected_rates_and_odd_preroll_use_whole_signed_phase() {
    let b = basis(1_000_000_000, 2);
    for (rate, initial, after) in [
        (Rate::new(1, 2).unwrap(), 99, 100),
        (Rate::NORMAL, 97, 101),
        (Rate::new(2, 1).unwrap(), 94, 102),
        (Rate::new(3, 2).unwrap(), 96, 101),
    ] {
        let t = transport(b, 100, 3, rate, None);
        assert_eq!(t.output_phase_basis(), Some(b));
        assert_eq!(
            t.position_at_output_phase(p(b, 100, 2)).unwrap(),
            ts(initial)
        );
        assert_eq!(t.position_at_output_phase(p(b, 104, 6)).unwrap(), ts(after));
    }
}

#[test]
fn mismatched_basis_generation_cut_and_domain_do_not_query_same_attempt() {
    let b = basis(44_100, 2);
    let t = transport(b, 0, 0, Rate::NORMAL, None);
    for other in [
        OutputPhaseBasis::new(ClockDomainId(1), 18, 3, 2, 44_100).unwrap(),
        OutputPhaseBasis::new(ClockDomainId(1), 17, 4, 2, 44_100).unwrap(),
        basis(44_100, 3),
        basis(48_000, 2),
        OutputPhaseBasis::new(ClockDomainId(9), 17, 3, 2, 44_100).unwrap(),
    ] {
        let q = other
            .point(
                ClockPoint {
                    domain: other.domain(),
                    timestamp: ts(100),
                },
                source(3, 0, 1),
            )
            .unwrap();
        assert!(t.position_at_output_phase(q).is_err());
    }
    assert!(t.position_at_output_phase(p(b, 99, 2)).is_err());
    assert!(t.position_at(ts(100)).is_err());
}

#[test]
fn finite_cap_does_not_turn_uncapped_song_overflow_into_wrapping() {
    let b = basis(1, 0);
    let uncapped = transport(b, 0, 0, Rate::new(2, 1).unwrap(), None);
    assert!(uncapped
        .position_at_output_phase(p(b, 101, u64::MAX))
        .is_err());
    let capped = transport(b, 0, 0, Rate::new(2, 1).unwrap(), Some(70));
    assert_eq!(
        capped
            .position_at_output_phase(p(b, 101, u64::MAX))
            .unwrap(),
        ts(70)
    );
    assert!(Transport::new_with_output_phase(
        b,
        clock(100),
        Timestamp::MIN,
        Duration::from_nanos(1),
        Rate::NORMAL,
        None
    )
    .is_err());
}

#[test]
fn nonzero_cut_pause_resume_history_and_noop_keep_phase() {
    let b = basis(3, 2);
    let mut t = transport(b, 0, 1, Rate::new(3, 2).unwrap(), None);
    let first = p(b, 110, 3);
    t.pause_at_output_phase(first).unwrap();
    assert!(t.is_paused());
    assert_eq!(
        t.position_at_output_phase(p(b, 115, 3)).unwrap(),
        ts(499_999_998)
    );
    assert!(t.position_at_output_phase(p(b, 115, 4)).is_err());
    t.pause_at_output_phase(p(b, 116, 3)).unwrap();
    assert_eq!(t.anchors().len(), 2);
    t.resume_at_output_phase(p(b, 120, 3)).unwrap();
    t.resume_at_output_phase(p(b, 121, 3)).unwrap();
    assert_eq!(t.anchors().len(), 3);
    assert_eq!(
        t.position_at_output_phase(p(b, 130, 4)).unwrap(),
        ts(999_999_997)
    );
    assert_eq!(
        t.position_at_output_phase(p(b, 105, 3)).unwrap(),
        ts(499_999_998)
    );
    assert_eq!(
        t.position_at_output_phase(p(b, 115, 3)).unwrap(),
        ts(499_999_998)
    );
    let snapshot = t.clone();
    assert!(t.pause_at_output_phase(p(b, 122, 2)).is_err());
    assert_eq!(t, snapshot);
    assert!(t.pause_at_output_phase(p(b, 119, 3)).is_err());
    assert_eq!(t, snapshot);
}

#[test]
fn legacy_controls_refuse_without_silently_retaining_phase() {
    let b = basis(44_100, 2);
    let mut t = transport(b, 0, 0, Rate::NORMAL, None);
    let snapshot = t.clone();
    assert!(t.seek(ts(110), ts(5)).is_err());
    assert!(t.set_rate(ts(110), Rate::new(2, 1).unwrap()).is_err());
    assert!(t.pause(ts(110)).is_err());
    assert!(t.resume(ts(110)).is_err());
    assert_eq!(t, snapshot);
    let legacy = Transport::new(ts(100), ts(0), Rate::NORMAL);
    assert!(legacy.position_at_output_phase(p(b, 110, 3)).is_err());
    assert_eq!(legacy.position_at(ts(110)).unwrap(), ts(10));
}

#[test]
fn query_and_fraction_arithmetic_allocate_zero_with_calibrated_probe() {
    TRACK.with(|t| t.set(true));
    let allocation = Box::new(123u64);
    std::hint::black_box(&allocation);
    TRACK.with(|t| t.set(false));
    assert!(ALLOCS.with(Cell::get) > 0);
    let b = basis(44_100, 2);
    let t = transport(b, 0, 1, Rate::new(3, 2).unwrap(), None);
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|v| v.set(true));
    for _ in 0..1000 {
        let q = b.point(clock(110), source(2, 121, 160)).unwrap();
        std::hint::black_box(t.position_at_output_phase(q).unwrap());
    }
    TRACK.with(|v| v.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
}

#[test]
fn historical_noop_controls_bound_elapsed_on_both_sides_even_when_song_is_equal() {
    let b = basis(1000, 2);
    let mut t = transport(b, 0, 3, Rate::new(1, u64::MAX).unwrap(), None);
    t.resume_at_output_phase(p(b, 110, 3)).unwrap();
    t.resume_at_output_phase(p(b, 120, 4)).unwrap();
    assert_eq!(
        t.anchors().len(),
        1,
        "running resume controls remain no-ops"
    );
    let snapshot = t.clone();
    // At h115, known active elapsed must lie between h110's 1ms and h120's 2ms.
    // This statement concerns acknowledged phase chronology, even though every
    // candidate maps to the identical integer song timestamp at this tiny rate.
    let below_prior = t.position_at_output_phase(p(b, 115, 2));
    let above_future = t.position_at_output_phase(p(b, 115, 5));
    assert_eq!(t, snapshot, "historical queries never mutate controls");
    assert_eq!(
        (below_prior.is_err(), above_future.is_err()),
        (true, true),
        "historical phase must respect both adjacent acknowledged no-op controls"
    );
    let valid = b.point(clock(115), source(3, 1, 2)).unwrap();
    assert_eq!(valid.elapsed_nanos(), 1_500_000);
    assert_eq!(t.position_at_output_phase(valid).unwrap(), ts(0));
    assert_eq!(t, snapshot);
}

#[test]
fn historical_running_query_cannot_exceed_later_acknowledged_pause_elapsed() {
    let b = basis(1000, 2);
    let mut t = transport(b, 0, 3, Rate::new(1, u64::MAX).unwrap(), None);
    t.resume_at_output_phase(p(b, 110, 3)).unwrap();
    t.pause_at_output_phase(p(b, 120, 4)).unwrap();
    assert!(t.is_paused());
    let snapshot = t.clone();
    // A historical running sample cannot have accrued 3ms before a subsequent
    // pause acknowledged only 2ms. The original integer song value is still 0.
    let impossible = t.position_at_output_phase(p(b, 115, 5));
    assert_eq!(t, snapshot);
    assert!(
        impossible.is_err(),
        "later pause must bound historical active elapsed"
    );
    assert_eq!(t.position_at_output_phase(p(b, 115, 3)).unwrap(), ts(0));
    assert_eq!(t.position_at_output_phase(p(b, 125, 4)).unwrap(), ts(0));
    assert_eq!(t, snapshot);
}

#[test]
fn direct_elapsed_builder_preserves_literals_and_matches_exact_frame_points() {
    for (hz, position, elapsed) in [
        (44_100, source(2, 121, 160), 17_148),
        (48_000, source(2, 26, 147), 3_684),
        (44_100, source(3, 0, 1), 22_675),
    ] {
        let b = basis(hz, 2);
        let direct = b.point_from_elapsed_nanos(clock(110), elapsed).unwrap();
        let frames = b.point(clock(110), position).unwrap();
        assert_eq!(direct, frames);
        assert_eq!(direct.logical(), clock(110));
        assert_eq!(direct.basis(), b);
        assert_eq!(direct.elapsed_nanos(), elapsed);
        let t = transport(b, 0, 1, Rate::new(3, 2).unwrap(), None);
        assert_eq!(
            t.position_at_output_phase(direct),
            t.position_at_output_phase(frames)
        );
        assert!(b.point_from_elapsed_nanos(clock(110), -1).is_err());
        assert!(b
            .point_from_elapsed_nanos(
                ClockPoint {
                    domain: ClockDomainId(9),
                    timestamp: ts(110)
                },
                elapsed
            )
            .is_err());
    }
    // A direct elapsed descriptor is arithmetic only. Same-domain identity is
    // validated here; native admission and the origin of E remain caller duties.
}

#[test]
fn largest_direct_elapsed_is_valid_math_but_song_conversion_remains_checked() {
    let b = basis(44_100, 2);
    let q = b.point_from_elapsed_nanos(clock(110), i128::MAX).unwrap();
    assert_eq!(q.elapsed_nanos(), i128::MAX);
    assert_eq!(q.basis(), b);
    assert_eq!(q.logical(), clock(110));
    let uncapped = transport(b, 0, 0, Rate::new(2, 1).unwrap(), None);
    assert!(uncapped.position_at_output_phase(q).is_err());
    let capped = transport(b, 0, 0, Rate::new(2, 1).unwrap(), Some(70));
    assert_eq!(capped.position_at_output_phase(q).unwrap(), ts(70));
}

#[test]
fn fixed_phase_controls_at_equal_logical_ns_keep_distinct_elapsed_history() {
    let b = basis(1_000_000_000, 2);
    let mut t = transport(b, 0, 1, Rate::new(1, 2).unwrap(), None);
    // Two actual acknowledgements can share the same rounded logical timestamp.
    // The no-op still constrains E; the later pause must not shadow earlier E.
    t.resume_at_output_phase(p(b, 110, 4)).unwrap(); // E=2
    t.pause_at_output_phase(p(b, 110, 6)).unwrap(); // E=4
    assert_eq!(t.anchors().len(), 2);
    for (relative, expected) in [(0, 0), (1, 0), (2, 0), (3, 1), (4, 1)] {
        assert_eq!(
            t.position_at_output_phase(p(b, 110, 2 + relative)).unwrap(),
            ts(expected),
            "fixed rate history must distinguish equal-ns cut E={relative}"
        );
    }
    let snapshot = t.clone();
    assert_eq!(
        t.pause_at_output_phase(p(b, 110, 5)),
        Err(beatkernel::transport::OutputPhaseError::RegressingElapsed),
        "historical E=3 is queryable but cannot mutate after E=4"
    );
    assert_eq!(t, snapshot);
    assert_eq!(
        t.resume_at_output_phase(p(b, 110, 5)),
        Err(beatkernel::transport::OutputPhaseError::RegressingElapsed)
    );
    assert_eq!(t, snapshot);
    t.resume_at_output_phase(p(b, 110, 6)).unwrap();
    assert_eq!(t.anchors().len(), 3);
    assert_eq!(t.position_at_output_phase(p(b, 110, 5)).unwrap(), ts(1));
    assert_eq!(t.position_at_output_phase(p(b, 111, 7)).unwrap(), ts(2));
    assert_eq!(
        t.runtime_output_rate(),
        None,
        "fixed constructor stays fixed"
    );
}
