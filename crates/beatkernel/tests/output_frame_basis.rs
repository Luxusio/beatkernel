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
