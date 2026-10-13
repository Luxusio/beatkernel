//! Generated span transfer is arithmetic data, never native admission evidence.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
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
// SAFETY: Every allocation argument passes unchanged to System; scalar TLS only.
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
    COUNTS.with(|value| value.set([0; 3]));
    TRACK.with(|value| value.set(true));
    let result = f();
    TRACK.with(|value| value.set(false));
    (result, COUNTS.with(Cell::get))
}
fn time(n: u64, d: u64) -> TargetTime {
    TargetTime::new(0, n, d).unwrap()
}
fn format(hz: u32) -> AudioFormat {
    AudioFormat::new(hz, 1).unwrap()
}
fn owner(source: u32, target: u32, prefix: usize) -> ConvertedMixer {
    let pcm = PcmLimits::new(1024, 1024, 1).unwrap();
    let bank = SampleBank::new(format(source), pcm).unwrap();
    let (_, consumer) = command_queue(8).unwrap();
    let limits = AudioLimits::new(8, 4, 8, 64, 8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(format(source), ClockDomainId(3), Timestamp::ZERO, limits),
        bank,
        consumer,
    )
    .unwrap();
    if prefix != 0 {
        mixer.render(&mut [0.0; 16][..prefix]).unwrap();
    }
    let mut owner = ConvertedMixer::new(
        mixer,
        format(target),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|failure| panic!("fixture refused: {:?}", failure.error()));
    owner.enable_phase_history(8).unwrap();
    owner
}
fn render(owner: &mut ConvertedMixer, n: usize) -> ConvertedRenderReport {
    owner.render(&mut [0.0; 16][..n]).unwrap()
}
fn hold(owner: &mut ConvertedMixer, n: usize) -> ConvertedRenderReport {
    owner.render_held(&mut [0.0; 16][..n]).unwrap()
}
fn retarget(owner: &mut ConvertedMixer, hz: u32) {
    owner
        .retarget(format(hz), ChannelMatrix::default_mix(1, 1).unwrap(), 16)
        .unwrap();
}
fn decoded(owner: &ConvertedMixer, index: usize) -> ConvertedPhaseSpan {
    ConvertedPhaseSpan::from_words(
        owner
            .phase_history()
            .unwrap()
            .span(index)
            .unwrap()
            .to_words(),
    )
    .unwrap()
}

#[test]
fn cold_seed_exposes_original_nonzero_coordinates_without_fabricating_a_span() {
    let mut owner = owner(3, 3, 2);
    let history = owner.phase_history().unwrap();
    assert_eq!(history.source_rate(), 3);
    assert_eq!(
        history.initial_source_position(),
        SourcePosition {
            frame: 2,
            numerator: 0,
            denominator: 1
        }
    );
    assert_eq!(history.start_time(), time(2, 3));
    assert!(history.span(0).is_none());
    assert!(history.span(usize::MAX).is_none());
    render(&mut owner, 0);
    hold(&mut owner, 0);
    assert!(owner.phase_history().unwrap().span(0).is_none());
    render(&mut owner, 1);
    assert_eq!(decoded(&owner, 0).to_words(), [3, 2, 0, 1, 0, 2, 3, 0]);
}

#[test]
fn active_roundtrip_keeps_forward_fraction_until_one_elapsed_floor() {
    let mut owner = owner(44_100, 48_000, 0);
    let report = render(&mut owner, 3);
    let span = decoded(&owner, 0);
    assert_eq!(span.to_words(), [44_100, 0, 0, 1, 0, 0, 1, 0]);
    let point = span
        .point(report.target_end_time, report.target_end_time)
        .unwrap();
    assert_eq!(point.source_rate(), 44_100);
    assert_eq!(point.state(), ConvertedOutputState::Active);
    assert_eq!(point.target_time(), time(1, 16_000));
    assert_eq!(point.elapsed_nanos_since(2), Ok(17_148));
}

#[test]
fn active_roundtrip_keeps_reverse_fraction_until_one_elapsed_floor() {
    let mut owner = owner(48_000, 44_100, 0);
    let report = render(&mut owner, 2);
    assert_eq!(
        decoded(&owner, 0)
            .point(report.target_end_time, report.target_end_time)
            .unwrap()
            .elapsed_nanos_since(2),
        Ok(3_684)
    );
}

#[test]
fn held_roundtrip_preserves_actual_fraction_and_frozen_source() {
    let mut owner = owner(44_100, 48_000, 0);
    let active = render(&mut owner, 4);
    let held = hold(&mut owner, 3);
    let span = decoded(&owner, 1);
    assert_eq!(span.to_words(), [44_100, 3, 27, 40, 0, 1, 12_000, 1]);
    for query in [active.target_end_time, time(1, 9_600), held.target_end_time] {
        let point = span.point(query, held.target_end_time).unwrap();
        assert_eq!(point.state(), ConvertedOutputState::Held);
        assert_eq!(point.elapsed_nanos_since(3), Ok(15_306));
    }
}

#[test]
fn continuous_retarget_keeps_single_actual_active_span() {
    let mut owner = owner(44_100, 48_000, 0);
    render(&mut owner, 4);
    retarget(&mut owner, 44_100);
    let report = render(&mut owner, 1);
    assert_eq!(owner.phase_history().unwrap().len(), 1);
    assert!(owner.phase_history().unwrap().span(1).is_none());
    assert_eq!(
        decoded(&owner, 0)
            .point(report.target_end_time, report.target_end_time)
            .unwrap()
            .elapsed_nanos_since(4),
        Ok(15_306)
    );
}

#[test]
fn resumed_wide_fraction_roundtrip_does_not_narrow_to_source_position() {
    let mut owner = owner(1, 3, 0);
    render(&mut owner, 1);
    retarget(&mut owner, 5);
    hold(&mut owner, 1);
    retarget(&mut owner, 3);
    let end = render(&mut owner, 1).target_end_time;
    let span = decoded(&owner, 2);
    assert_eq!(span.to_words(), [1, 0, 1, 3, 0, 8, 15, 0]);
    let query = time(12_297_829_382_473_034_409, 18_446_744_073_709_551_614);
    let point = span.point(query, end).unwrap();
    assert_eq!(point.elapsed_nanos_since(0), Ok(466_666_666));
    assert_eq!(
        point.elapsed_nanos_since(1),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn high_limb_roundtrip_preserves_independent_rational_oracles() {
    let mut owner = owner(4_294_967_291, 4_294_967_279, 0);
    render(&mut owner, 1);
    retarget(&mut owner, 4_294_967_231);
    render(&mut owner, 1);
    hold(&mut owner, 1);
    let end = render(&mut owner, 16).target_end_time;
    let span = decoded(&owner, 2);
    // Rational oracle: physical duration = query - exactly one held targetB frame.
    for (numerator, zero, two) in [
        (13_958_643_875, 0, 0),
        (47_244_640_923, 2, 1),
        (80_530_637_970, 4, 3),
    ] {
        let point = span.point(time(numerator, u64::MAX), end).unwrap();
        assert_eq!(point.elapsed_nanos_since(0), Ok(zero));
        assert_eq!(point.elapsed_nanos_since(2), Ok(two));
    }
}

#[test]
fn malformed_words_refuse_rate_fraction_target_and_state() {
    let good = [44_100, 2, 121, 160, 0, 1, 16_000, 0];
    for (index, value) in [
        (0, 0),
        (0, u64::from(u32::MAX) + 1),
        (2, 160),
        (2, 161),
        (3, 0),
        (5, 16_000),
        (6, 0),
        (7, 2),
        (7, u64::MAX),
    ] {
        let mut words = good;
        words[index] = value;
        assert!(
            matches!(
                ConvertedPhaseSpan::from_words(words),
                Err(ConvertedPhaseError::InvalidDescriptor)
            ),
            "field {index}, value {value}"
        );
    }
}

#[test]
fn structurally_valid_unreduced_fractions_keep_elapsed_value() {
    let span = ConvertedPhaseSpan::from_words([3, 1, 2, 4, 0, 2, 6, 0]).unwrap();
    let point = span.point(time(1, 2), time(1, 2)).unwrap();
    assert_eq!(point.elapsed_nanos_since(1), Ok(333_333_333));
}

#[test]
fn exact_range_guards_distinguish_before_after_and_invalid_end() {
    let span = ConvertedPhaseSpan::from_words([3, 2, 0, 1, 0, 2, 3, 0]).unwrap();
    let start = time(2, 3);
    let end = TargetTime::new(1, 0, 1).unwrap();
    assert_eq!(
        span.point(start, start).unwrap().elapsed_nanos_since(2),
        Ok(0)
    );
    assert_eq!(
        span.point(end, end).unwrap().elapsed_nanos_since(2),
        Ok(333_333_333)
    );
    assert!(matches!(
        span.point(time(1, 3), end),
        Err(ConvertedPhaseError::BeforeRetainedRange)
    ));
    assert!(matches!(
        span.point(TargetTime::new(1, 1, u64::MAX).unwrap(), end),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    ));
    assert!(matches!(
        span.point(start, time(1, 3)),
        Err(ConvertedPhaseError::InvalidRange)
    ));
}

#[test]
fn decoded_data_does_not_authenticate_an_epoch_or_submitted_bound() {
    // The codec deliberately accepts mathematical data without owner/epoch/native evidence.
    // An adapter must separately validate those facts: point only enforces supplied bounds.
    let span = ConvertedPhaseSpan::from_words([1, 0, 0, 1, 0, 0, 1, 0]).unwrap();
    assert_eq!(
        span.point(time(1, 2), time(1, 2))
            .unwrap()
            .elapsed_nanos_since(0),
        Ok(500_000_000)
    );
    assert!(matches!(
        span.point(time(1, 2), time(1, 3)),
        Err(ConvertedPhaseError::AfterGeneratedRange)
    ));
}

#[test]
fn fixed_word_roundtrip_and_point_query_allocate_nothing() {
    let mut owner = owner(44_100, 48_000, 0);
    let end = render(&mut owner, 3).target_end_time;
    let span = owner.phase_history().unwrap().span(0).unwrap();
    let (elapsed, counts) = tracked(|| {
        let words = std::hint::black_box(span.to_words());
        ConvertedPhaseSpan::from_words(words)
            .unwrap()
            .point(end, end)
            .unwrap()
            .elapsed_nanos_since(2)
    });
    assert_eq!(elapsed, Ok(17_148));
    assert_eq!(counts, [0; 3]);
}

fn origin() -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(7),
        timestamp: Timestamp::from_nanos(-100),
    }
}
#[test]
fn native_counter_combines_nonzero_fraction_before_timestamp_floor() {
    let basis = TargetFrameBasis::new(origin(), time(1, 3), 48_000).unwrap();
    let exact = basis.time_at_native_counter(1, 7).unwrap();
    assert_eq!(exact, time(10, 21));
    assert_eq!(
        exact.point(origin()).unwrap().timestamp,
        Timestamp::from_nanos(476_190_376)
    );
    assert_eq!(basis.point_at_native_counter(1, 7), exact.point(origin()));
    assert_eq!(basis.time_at_native_counter(0, u64::MAX), Ok(time(1, 3)));
}

#[test]
fn native_counter_large_frequency_preserves_fraction_and_refuses_overwide_denominator() {
    let basis = TargetFrameBasis::new(origin(), time(0, 1), 1).unwrap();
    assert_eq!(
        basis.time_at_native_counter(u64::MAX - 1, u64::MAX),
        Ok(time(u64::MAX - 1, u64::MAX))
    );
    assert_eq!(
        basis.time_at_native_counter(1, 0),
        Err(AudioError::InvalidFormat)
    );
    let fractional = TargetFrameBasis::new(origin(), time(1, 3), 3).unwrap();
    assert_eq!(
        fractional.time_at_native_counter(1, u64::MAX - 1),
        Err(AudioError::Overflow)
    );
    assert_eq!(
        fractional.point_at_native_counter(1, u64::MAX - 1),
        Err(AudioError::Overflow)
    );
}

#[test]
fn exact_native_time_is_not_artificially_limited_by_clock_timestamp_range() {
    let basis = TargetFrameBasis::new(origin(), time(0, 1), 1).unwrap();
    assert_eq!(
        basis.time_at_native_counter(u64::MAX, 1),
        Ok(TargetTime::new(u64::MAX, 0, 1).unwrap())
    );
    assert_eq!(
        basis.point_at_native_counter(u64::MAX, 1),
        Err(AudioError::Overflow)
    );
}
