//! Deferred original physical mixer-grid basis and independent rational arithmetic.
use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
};
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(u32::MAX),
        timestamp: Timestamp::from_nanos(ns),
    }
}
#[test]
fn bounded_frame_and_arbitrary_counter_conversions_match_independent_common_denominator_reference()
{
    for rate in [1u32, 3, 44_100, 48_000, u32::MAX] {
        for offset in [0u64, 1, 2, 997] {
            let basis = OutputFrameBasis::new(point(-123), rate, offset).unwrap();
            for frequency in [1u64, 3, 1000, 10_000_000] {
                for position in [0u64, 1, 2, 911] {
                    let numerator = u128::from(offset) * 1_000_000_000 * u128::from(frequency)
                        + u128::from(position) * 1_000_000_000 * u128::from(rate);
                    let expected =
                        -123i128 + (numerator / (u128::from(rate) * u128::from(frequency))) as i128;
                    assert_eq!(
                        basis.point_at_native_counter(position, frequency).unwrap(),
                        point(i64::try_from(expected).unwrap())
                    );
                }
            }
            for frame in [0u64, 1, 9] {
                let expected = -123i128
                    + (u128::from(offset + frame) * 1_000_000_000 / u128::from(rate)) as i128;
                assert_eq!(
                    basis.point_at_stream_frame(frame).unwrap(),
                    point(i64::try_from(expected).unwrap())
                );
            }
        }
    }
}
#[test]
fn fractional_carry_is_added_before_floor_and_extreme_quotients_do_not_overflow_common_numerator() {
    let basis = OutputFrameBasis::new(point(0), 3, 2).unwrap();
    assert_eq!(
        basis.point_at_stream_frame(1).unwrap(),
        point(1_000_000_000)
    );
    assert_eq!(
        basis.point_at_native_counter(1, 3).unwrap(),
        point(1_000_000_000)
    );
    assert_eq!(
        basis
            .point_at_native_counter(u64::MAX - 1, u64::MAX)
            .unwrap(),
        point(1_666_666_666)
    );
    let extreme = OutputFrameBasis::new(point(0), u32::MAX, u64::MAX).unwrap();
    assert_eq!(
        extreme.point_at_native_counter(u64::MAX, u64::MAX).unwrap(),
        point(4_294_967_298_000_000_000)
    );
    let negative = OutputFrameBasis::new(point(i64::MIN), u32::MAX, u64::MAX).unwrap();
    assert_eq!(
        negative
            .point_at_native_counter(u64::MAX, u64::MAX)
            .unwrap(),
        point(-4_928_404_738_854_775_808)
    );
}
#[test]
fn zero_rates_frequencies_and_checked_timestamp_or_frame_overflow_refuse_without_changing_basis() {
    assert_eq!(
        OutputFrameBasis::new(point(0), 0, 0),
        Err(OutputFrameBasisError::InvalidRate)
    );
    let basis = OutputFrameBasis::new(point(i64::MAX), 3, 0).unwrap();
    assert_eq!(
        basis.point_at_native_counter(1, 0),
        Err(OutputFrameBasisError::InvalidFrequency)
    );
    assert_eq!(
        basis.point_at_stream_frame(1),
        Err(OutputFrameBasisError::Overflow)
    );
    assert_eq!(
        (
            basis.origin(),
            basis.sample_rate(),
            basis.start_physical_frame()
        ),
        (point(i64::MAX), 3, 0)
    );
    let full = OutputFrameBasis::new(point(0), 1, u64::MAX).unwrap();
    assert_eq!(
        full.point_at_stream_frame(1),
        Err(OutputFrameBasisError::Overflow)
    );
    assert_eq!(
        full.point_at_native_counter(0, 1),
        Err(OutputFrameBasisError::Overflow)
    );
}
#[test]
fn actual_paused_mixer_captures_physical_cursor_without_mutating_queue_or_playback_state() {
    let format = AudioFormat::new(3, 1).unwrap();
    let bank = SampleBank::new(format, PcmLimits::new(64, 128, 1).unwrap()).unwrap();
    let (mut producer, consumer) = command_queue(8).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            point(-1_000_000_000).domain,
            point(-1_000_000_000).timestamp,
            AudioLimits::new(8, 2, 8, 8, 8).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    mixer.render(&mut [0.; 2]).unwrap();
    producer.request_pause(true);
    mixer.render(&mut [0.; 3]).unwrap();
    let before = (
        mixer.frame_cursor(),
        mixer.playback_frame_cursor(),
        mixer.counters(),
        mixer.rate(),
        mixer.is_paused(),
    );
    let basis = mixer.output_frame_basis();
    assert_eq!(
        (
            basis.origin(),
            basis.sample_rate(),
            basis.start_physical_frame()
        ),
        (point(-1_000_000_000), 3, 5)
    );
    assert_eq!(
        basis.point_at_stream_frame(1).unwrap(),
        point(1_000_000_000)
    );
    assert_eq!(
        (mixer.frame_cursor(), mixer.playback_frame_cursor()),
        (5, 2)
    );
    assert_eq!(
        (
            mixer.frame_cursor(),
            mixer.playback_frame_cursor(),
            mixer.counters(),
            mixer.rate(),
            mixer.is_paused()
        ),
        before
    );
}

#[test]
fn target_basis_maps_only_admitted_prefix_using_exact_piecewise_start() {
    const DEN: u128 = 14_112_000;
    let origin = point(-444_333_222);
    let start = TargetTime::from_frames(17, 44_100)
        .unwrap()
        .checked_add_frames(7, 48_000)
        .unwrap();
    let basis = TargetFrameBasis::new(origin, start, 32_000).unwrap();
    assert_eq!(
        (basis.origin(), basis.start_time(), basis.sample_rate()),
        (origin, start, 32_000)
    );
    let start_ticks = 17 * (DEN / 44_100) + 7 * (DEN / 48_000);
    // An eight-frame generated block can admit only two. Its next stream must
    // start at that two-frame prefix, never at the source lookahead or block end.
    for prefix in [0, 1, 2, 8] {
        let ticks = start_ticks + u128::from(prefix) * (DEN / 32_000);
        let expected = -444_333_222 + (ticks * 1_000_000_000 / DEN) as i64;
        assert_eq!(
            basis.point_at_stream_frame(prefix).unwrap(),
            point(expected)
        );
        assert_eq!(
            basis
                .time_at_stream_frame(prefix)
                .unwrap()
                .point(origin)
                .unwrap(),
            point(expected)
        );
    }
    let next =
        TargetFrameBasis::new(origin, basis.time_at_stream_frame(2).unwrap(), 48_000).unwrap();
    let ticks = start_ticks + 2 * (DEN / 32_000) + 1 * (DEN / 48_000);
    assert_eq!(
        next.point_at_stream_frame(1).unwrap(),
        point(-444_333_222 + (ticks * 1_000_000_000 / DEN) as i64)
    );
}

#[test]
fn target_basis_native_counter_adds_fractions_before_floor_and_keeps_original_domain() {
    let start = TargetTime::new(0, 2, 3).unwrap();
    let basis = TargetFrameBasis::new(point(-5), start, 48_000).unwrap();
    assert_eq!(
        basis.point_at_native_counter(1, 3).unwrap(),
        point(999_999_995)
    );
    for frequency in [3_u64, 1_000, 10_000_000] {
        for ticks in [0_u64, 1, 911] {
            let numerator = 2_u128 * u128::from(frequency) + 3 * u128::from(ticks);
            let denominator = 3_u128 * u128::from(frequency);
            assert_eq!(
                basis.point_at_native_counter(ticks, frequency).unwrap(),
                point(-5 + (numerator * 1_000_000_000 / denominator) as i64)
            );
        }
    }
    assert!(basis.point_at_native_counter(1, 0).is_err());
    assert_eq!(basis.start_time(), start);
    assert!(TargetFrameBasis::new(point(0), start, 0).is_err());
    let zero = TargetTime::new(0, 0, 1).unwrap();
    let overflow = TargetFrameBasis::new(point(i64::MAX), zero, 48_000).unwrap();
    assert!(overflow.point_at_stream_frame(1).is_err());
    assert_eq!(overflow.start_time(), zero);
}
