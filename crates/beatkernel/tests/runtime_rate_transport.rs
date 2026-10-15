//! Actual caller-qualified runtime cuts retain fractional song phase.
//! These pure fixtures do not prove native presentation acknowledgement.
use beatkernel::{
    audio::SourcePosition,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
    transport::{
        LiveTransportSetupError, OutputPhaseBasis, OutputPhaseError, OutputPhasePoint,
        PositiveRatePhaseError, Rate, Transport, TransportError,
    },
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
    static REALLOCS: Cell<usize> = const { Cell::new(0) };
    static DEALLOCS: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
// SAFETY: Original arguments are forwarded unchanged to System. Instrumentation
// uses only allocation-free thread-local scalar cells.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|t| {
            if t.get() {
                ALLOCS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|t| {
            if t.get() {
                REALLOCS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        TRACK.with(|t| {
            if t.get() {
                DEALLOCS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.dealloc(ptr, layout) }
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
fn rate(n: i64, d: u64) -> Rate {
    Rate::new(n, d).unwrap()
}
fn basis(hz: u32) -> OutputPhaseBasis {
    OutputPhaseBasis::new(ClockDomainId(1), 17, 3, 2, hz).unwrap()
}
fn frame(b: OutputPhaseBasis, host: i64, relative: u64) -> OutputPhasePoint {
    b.point(
        clock(host),
        SourcePosition {
            frame: b.active_origin_frame() + relative,
            numerator: 0,
            denominator: 1,
        },
    )
    .unwrap()
}
fn elapsed(b: OutputPhaseBasis, host: i64, nanos: i128) -> OutputPhasePoint {
    b.point_from_elapsed_nanos(clock(host), nanos).unwrap()
}
fn transport(
    b: OutputPhaseBasis,
    start: i64,
    preroll: i64,
    initial: Rate,
    end: Option<i64>,
) -> Transport {
    Transport::new_with_runtime_output_phase(
        b,
        clock(100),
        ts(start),
        Duration::from_nanos(preroll),
        initial,
        end.map(ts),
    )
    .unwrap()
}

#[test]
fn hz44100_runtime_cut_keeps_preroll_residue_through_later_frames() {
    let b = basis(44_100);
    let mut t = transport(b, 0, 3, rate(2, 3), None);
    let setup = t.preroll_setup();
    let cut = frame(b, 110, 1);
    assert_eq!(cut.elapsed_nanos(), 22_675);
    assert_eq!(t.position_at_output_phase(cut).unwrap(), ts(15_114));
    t.set_rate_at_output_phase(cut, rate(1, 2)).unwrap();
    assert_eq!(t.runtime_output_rate(), Some(rate(1, 2)));
    assert_eq!(t.output_phase_basis(), Some(b));
    assert_eq!(t.preroll_setup(), setup, "cold invocation is immutable");
    assert_eq!(t.anchors().len(), 2);
    assert_eq!(t.anchor().song_time, ts(15_114));
    for (f, expected) in [(2, 26_452), (3, 37_790), (4, 49_128)] {
        assert_eq!(
            t.position_at_output_phase(frame(b, 110 + f as i64, f))
                .unwrap(),
            ts(expected)
        );
    }
    assert_eq!(
        t.position_at_output_phase(frame(b, 105, 0)).unwrap(),
        ts(-2),
        "late pre-cut queries retain original phase"
    );
}

#[test]
fn hz48000_runtime_cut_does_not_round_away_half_a_nanosecond() {
    let b = basis(48_000);
    let mut t = transport(b, 0, 3, rate(1, 2), None);
    let cut = frame(b, 110, 2);
    assert_eq!(cut.elapsed_nanos(), 41_666);
    assert_eq!(t.position_at_output_phase(cut).unwrap(), ts(20_831));
    t.set_rate_at_output_phase(cut, rate(3, 2)).unwrap();
    for (f, expected) in [(3, 52_082), (4, 83_332), (5, 114_581)] {
        assert_eq!(
            t.position_at_output_phase(frame(b, 110 + f as i64, f))
                .unwrap(),
            ts(expected)
        );
    }
    assert_eq!(
        t.position_at_output_phase(frame(b, 105, 1)).unwrap(),
        ts(10_415)
    );
}

#[test]
fn normal_initial_rate_can_change_and_start_stays_outside_truncation() {
    let b = basis(1_000_000_000);
    let mut normal = transport(b, 100, 0, Rate::NORMAL, None);
    normal
        .set_rate_at_output_phase(frame(b, 110, 1), rate(1, 2))
        .unwrap();
    assert_eq!(normal.runtime_output_rate(), Some(rate(1, 2)));
    assert_eq!(
        normal.position_at_output_phase(frame(b, 120, 2)).unwrap(),
        ts(101)
    );

    let mut negative = transport(b, 100, 1, rate(1, 2), None);
    assert_eq!(
        negative.position_at_output_phase(frame(b, 100, 0)).unwrap(),
        ts(100),
        "S + trunc(-1/2) is100, not trunc(S-1/2)=99"
    );
    negative
        .set_rate_at_output_phase(frame(b, 100, 0), rate(3, 2))
        .unwrap();
    assert_eq!(
        negative.position_at_output_phase(frame(b, 110, 1)).unwrap(),
        ts(101)
    );
}

#[test]
fn equal_logical_timestamp_cuts_are_selected_by_exact_elapsed() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 1, rate(1, 2), None);
    t.set_rate_at_output_phase(elapsed(b, 110, 2), rate(3, 2))
        .unwrap();
    t.set_rate_at_output_phase(elapsed(b, 110, 4), rate(1, 2))
        .unwrap();
    assert_eq!(t.anchors().len(), 3);
    assert_eq!(t.anchors()[1].host_time, t.anchors()[2].host_time);
    for (e, expected) in [(0, 0), (1, 0), (2, 0), (3, 2), (4, 3), (5, 4)] {
        assert_eq!(
            t.position_at_output_phase(elapsed(b, 110, e)).unwrap(),
            ts(expected),
            "same logical timestamp, distinct exact E={e}"
        );
    }
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 109, 1)).unwrap(),
        ts(0)
    );
    let snapshot = t.clone();
    assert_eq!(
        t.set_rate_at_output_phase(elapsed(b, 110, 3), Rate::NORMAL),
        Err(OutputPhaseError::RegressingElapsed),
        "a valid historical query is not a valid mutating cut"
    );
    assert_eq!(t, snapshot);
    assert_eq!(
        t.pause_at_output_phase(elapsed(b, 110, 3)),
        Err(OutputPhaseError::RegressingElapsed)
    );
    assert_eq!(t, snapshot);
}

#[test]
fn serial_changes_at_same_exact_cut_keep_earlier_phase_and_latest_rate() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 1, rate(1, 2), None);
    let cut = elapsed(b, 110, 2);
    t.set_rate_at_output_phase(cut, rate(3, 2)).unwrap();
    t.set_rate_at_output_phase(cut, rate(2, 1)).unwrap();
    assert_eq!(t.runtime_output_rate(), Some(rate(2, 1)));
    assert_eq!(t.position_at_output_phase(cut).unwrap(), ts(0));
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 110, 1)).unwrap(),
        ts(0)
    );
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 111, 3)).unwrap(),
        ts(2)
    );
}

#[test]
fn normalized_noop_adds_no_anchor_but_advances_host_and_elapsed_bounds() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 1, rate(1, 2), None);
    let setup = t.preroll_setup();
    let original_anchor = t.anchor();
    t.set_rate_at_output_phase(elapsed(b, 110, 2), rate(2, 4))
        .unwrap();
    t.set_rate_at_output_phase(elapsed(b, 120, 4), rate(3, 6))
        .unwrap();
    assert_eq!(t.anchors(), &[original_anchor]);
    assert_eq!(t.preroll_setup(), setup);
    assert_eq!(t.runtime_output_rate(), Some(rate(1, 2)));
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 115, 3)).unwrap(),
        ts(1)
    );
    let snapshot = t.clone();
    for e in [1, 5] {
        assert_eq!(
            t.position_at_output_phase(elapsed(b, 115, e)),
            Err(OutputPhaseError::RegressingElapsed)
        );
        assert_eq!(t, snapshot);
    }
    assert_eq!(
        t.set_rate_at_output_phase(elapsed(b, 121, 3), rate(1, 2)),
        Err(OutputPhaseError::RegressingElapsed)
    );
    assert_eq!(t, snapshot);
    assert_eq!(
        t.set_rate_at_output_phase(elapsed(b, 119, 4), rate(1, 2)),
        Err(OutputPhaseError::Transport(
            TransportError::NonMonotonicHost
        ))
    );
    assert_eq!(t, snapshot);
}

#[test]
fn repeated_noop_at_identical_point_changes_nothing() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 1, rate(1, 2), None);
    let cut = elapsed(b, 110, 2);
    t.set_rate_at_output_phase(cut, rate(2, 4)).unwrap();
    let snapshot = t.clone();
    t.set_rate_at_output_phase(cut, rate(3, 6)).unwrap();
    assert_eq!(t, snapshot);
}

#[test]
fn pause_resume_keeps_changed_rate_fraction_and_original_setup() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 100, 1, rate(1, 2), None);
    let setup = t.preroll_setup();
    t.set_rate_at_output_phase(elapsed(b, 110, 2), rate(3, 2))
        .unwrap();
    t.pause_at_output_phase(elapsed(b, 120, 4)).unwrap();
    assert!(t.is_paused());
    assert_eq!(t.runtime_output_rate(), Some(rate(3, 2)));
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 125, 4)).unwrap(),
        ts(103)
    );
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 125, 5)),
        Err(OutputPhaseError::InvalidState)
    );
    let paused = t.clone();
    assert_eq!(
        t.set_rate_at_output_phase(elapsed(b, 125, 4), Rate::NORMAL),
        Err(OutputPhaseError::InvalidState)
    );
    assert_eq!(t, paused);
    t.pause_at_output_phase(elapsed(b, 130, 4)).unwrap();
    assert_eq!(t.anchors().len(), 3);
    t.resume_at_output_phase(elapsed(b, 140, 4)).unwrap();
    t.resume_at_output_phase(elapsed(b, 145, 4)).unwrap();
    assert!(!t.is_paused());
    assert_eq!(t.anchor().rate, rate(3, 2));
    assert_eq!(t.anchors().len(), 4);
    assert_eq!(t.preroll_setup(), setup);
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 150, 5)).unwrap(),
        ts(105),
        "resume retains3+1/2 rather than integer reanchoring at3"
    );
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 115, 3)).unwrap(),
        ts(102)
    );
}

#[test]
fn duplicate_pause_requires_exact_frozen_elapsed_without_mutation() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 0, Rate::NORMAL, None);
    t.pause_at_output_phase(elapsed(b, 110, 3)).unwrap();
    let snapshot = t.clone();
    assert_eq!(
        t.pause_at_output_phase(elapsed(b, 120, 4)),
        Err(OutputPhaseError::InvalidState)
    );
    assert_eq!(t, snapshot);
    assert_eq!(
        t.resume_at_output_phase(elapsed(b, 120, 4)),
        Err(OutputPhaseError::InvalidState)
    );
    assert_eq!(t, snapshot);
    t.pause_at_output_phase(elapsed(b, 120, 3)).unwrap();
    assert_eq!(t.anchors().len(), 2);
    t.resume_at_output_phase(elapsed(b, 130, 3)).unwrap();
    assert_eq!(t.runtime_output_rate(), Some(Rate::NORMAL));
    assert_eq!(
        t.position_at_output_phase(elapsed(b, 140, 4)).unwrap(),
        ts(4)
    );
}

#[test]
fn wrong_attempt_owner_generation_origin_hz_or_domain_is_atomic() {
    let b = basis(44_100);
    let mut t = transport(b, 0, 0, Rate::NORMAL, None);
    let snapshot = t.clone();
    for (domain, owner, generation, origin, hz) in [
        (1, 18, 3, 2, 44_100),
        (1, 17, 4, 2, 44_100),
        (1, 17, 3, 3, 44_100),
        (1, 17, 3, 2, 48_000),
        (9, 17, 3, 2, 44_100),
    ] {
        let other =
            OutputPhaseBasis::new(ClockDomainId(domain), owner, generation, origin, hz).unwrap();
        let q = other
            .point_from_elapsed_nanos(
                ClockPoint {
                    domain: ClockDomainId(domain),
                    timestamp: ts(110),
                },
                1,
            )
            .unwrap();
        let expected = if domain == 1 {
            OutputPhaseError::WrongBasis
        } else {
            OutputPhaseError::WrongDomain
        };
        assert_eq!(t.set_rate_at_output_phase(q, rate(3, 2)), Err(expected));
        assert_eq!(t, snapshot);
    }
}

#[test]
fn nonpositive_and_denominator_capacity_errors_preserve_every_field() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 0, Rate::NORMAL, None);
    let snapshot = t.clone();
    for invalid in [Rate::ZERO, Rate::REVERSE, rate(-3, 2)] {
        assert_eq!(
            t.set_rate_at_output_phase(elapsed(b, 110, 1), invalid),
            Err(OutputPhaseError::RatePhase(
                PositiveRatePhaseError::NonPositiveRate
            ))
        );
        assert_eq!(t, snapshot);
    }
    let mut tiny = transport(b, 0, 0, rate(1, u64::MAX), None);
    let before = tiny.clone();
    assert_eq!(
        tiny.set_rate_at_output_phase(elapsed(b, 110, 1), rate(1, 2)),
        Err(OutputPhaseError::RatePhase(
            PositiveRatePhaseError::DenominatorCapacity
        ))
    );
    assert_eq!(tiny, before);
    assert_eq!(
        tiny.position_at_output_phase(elapsed(b, 111, 2)).unwrap(),
        ts(0)
    );
    tiny.set_rate_at_output_phase(elapsed(b, 110, 1), rate(1, u64::MAX))
        .unwrap();
    assert_eq!(tiny.anchors().len(), 1);
}

#[test]
fn fixed_output_phase_and_scalar_require_explicit_runtime_opt_in() {
    let b = basis(1_000_000_000);
    let fixed =
        Transport::new_with_output_phase(b, clock(100), ts(0), Duration::ZERO, Rate::NORMAL, None)
            .unwrap();
    for mut t in [fixed, Transport::new(ts(100), ts(0), Rate::NORMAL)] {
        let before = t.clone();
        assert_eq!(t.runtime_output_rate(), None);
        assert_eq!(
            t.set_rate_at_output_phase(elapsed(b, 110, 1), rate(1, 2)),
            Err(OutputPhaseError::RuntimeRateRequired)
        );
        assert_eq!(t, before);
    }
    let mut scalar = Transport::new(ts(100), ts(0), Rate::NORMAL);
    assert_eq!(
        scalar.try_reserve_output_phase_controls(3),
        Err(OutputPhaseError::MissingPhase)
    );
}

#[test]
fn runtime_transport_still_refuses_legacy_scalar_controls() {
    let b = basis(44_100);
    let mut t = transport(b, 0, 0, Rate::NORMAL, None);
    let snapshot = t.clone();
    assert_eq!(
        t.position_at(ts(110)),
        Err(TransportError::OutputPhaseRequired)
    );
    assert_eq!(
        t.seek(ts(110), ts(5)),
        Err(TransportError::OutputPhaseRequired)
    );
    assert_eq!(
        t.set_rate(ts(110), rate(2, 1)),
        Err(TransportError::OutputPhaseRequired)
    );
    assert_eq!(t.pause(ts(110)), Err(TransportError::OutputPhaseRequired));
    assert_eq!(t.resume(ts(110)), Err(TransportError::OutputPhaseRequired));
    assert_eq!(t, snapshot);
}

#[test]
fn cold_runtime_constructor_preserves_existing_setup_refusals() {
    let b = basis(44_100);
    for (start, preroll, initial, end, expected) in [
        (
            0,
            -1,
            Rate::NORMAL,
            None,
            LiveTransportSetupError::NegativePreroll,
        ),
        (
            0,
            0,
            Rate::ZERO,
            None,
            LiveTransportSetupError::NonPositiveRate,
        ),
        (
            0,
            0,
            Rate::REVERSE,
            None,
            LiveTransportSetupError::NonPositiveRate,
        ),
        (
            5,
            0,
            Rate::NORMAL,
            Some(4),
            LiveTransportSetupError::InvalidEnd,
        ),
        (
            i64::MIN,
            1,
            Rate::NORMAL,
            None,
            LiveTransportSetupError::Overflow,
        ),
    ] {
        assert_eq!(
            Transport::new_with_runtime_output_phase(
                b,
                clock(100),
                ts(start),
                Duration::from_nanos(preroll),
                initial,
                end.map(ts)
            ),
            Err(OutputPhaseError::Setup(expected))
        );
    }
    assert_eq!(
        Transport::new_with_runtime_output_phase(
            b,
            ClockPoint {
                domain: ClockDomainId(9),
                timestamp: ts(100)
            },
            ts(0),
            Duration::ZERO,
            Rate::NORMAL,
            None
        ),
        Err(OutputPhaseError::WrongDomain)
    );
}

#[test]
fn finite_cap_survives_positive_wide_overflow_without_claiming_end_or_pause() {
    let b = basis(1_000_000_000);
    let mut capped = transport(b, 0, 0, rate(1, 2), Some(70));
    capped
        .set_rate_at_output_phase(elapsed(b, 110, 1), rate(i64::MAX, 1))
        .unwrap();
    assert_eq!(
        capped
            .position_at_output_phase(elapsed(b, 120, i128::MAX))
            .unwrap(),
        ts(70)
    );
    assert!(
        !capped.is_paused(),
        "a numeric cap cannot acknowledge completion"
    );
    assert_eq!(capped.runtime_output_rate(), Some(rate(i64::MAX, 1)));
    let mut uncapped = transport(b, 0, 0, rate(1, 2), None);
    uncapped
        .set_rate_at_output_phase(elapsed(b, 110, 1), rate(i64::MAX, 1))
        .unwrap();
    assert!(uncapped
        .position_at_output_phase(elapsed(b, 120, i128::MAX))
        .is_err());
}

#[test]
fn reservation_failure_is_typed_and_semantically_atomic() {
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 0, Rate::NORMAL, None);
    let snapshot = t.clone();
    assert_eq!(
        t.try_reserve_output_phase_controls(usize::MAX),
        Err(OutputPhaseError::AllocationFailed)
    );
    assert_eq!(t, snapshot);
    t.try_reserve_output_phase_controls(0).unwrap();
    assert_eq!(t, snapshot);
}

fn reset_counts() {
    ALLOCS.with(|n| n.set(0));
    REALLOCS.with(|n| n.set(0));
    DEALLOCS.with(|n| n.set(0));
}
fn counts() -> (usize, usize, usize) {
    (
        ALLOCS.with(Cell::get),
        REALLOCS.with(Cell::get),
        DEALLOCS.with(Cell::get),
    )
}

#[test]
fn reserved_rate_pause_resume_and_historical_queries_have_calibrated_zero_heap() {
    reset_counts();
    TRACK.with(|t| t.set(true));
    let mut calibration = Vec::<u64>::with_capacity(1);
    calibration.push(123);
    calibration.reserve_exact(16);
    std::hint::black_box(&calibration);
    drop(calibration);
    TRACK.with(|t| t.set(false));
    let (allocs, reallocs, deallocs) = counts();
    assert!(
        allocs > 0 && reallocs > 0 && deallocs > 0,
        "all heap probes must calibrate"
    );
    let b = basis(1_000_000_000);
    let mut t = transport(b, 0, 1, rate(1, 2), None);
    t.try_reserve_output_phase_controls(16).unwrap();
    let points = [
        elapsed(b, 110, 2),
        elapsed(b, 120, 3),
        elapsed(b, 130, 4),
        elapsed(b, 140, 4),
        elapsed(b, 150, 4),
        elapsed(b, 160, 5),
        elapsed(b, 170, 6),
        elapsed(b, 180, 7),
        elapsed(b, 115, 3),
    ];
    reset_counts();
    TRACK.with(|t| t.set(true));
    let mutations = [
        t.set_rate_at_output_phase(points[0], rate(3, 2)),
        t.set_rate_at_output_phase(points[1], rate(6, 4)),
        t.pause_at_output_phase(points[2]),
        t.pause_at_output_phase(points[3]),
        t.resume_at_output_phase(points[4]),
        t.resume_at_output_phase(points[5]),
        t.set_rate_at_output_phase(points[6], rate(1, 2)),
    ];
    let future = t.position_at_output_phase(points[7]);
    let past = t.position_at_output_phase(points[8]);
    for _ in 0..1000 {
        let _ = std::hint::black_box(t.position_at_output_phase(points[8]));
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(counts(), (0, 0, 0));
    assert!(mutations.into_iter().all(|result| result.is_ok()));
    assert_eq!(future.unwrap(), ts(7));
    assert_eq!(past.unwrap(), ts(2));
}
