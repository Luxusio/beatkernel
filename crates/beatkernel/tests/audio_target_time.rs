use beatkernel::{
    audio::*,
    time::{ClockDomainId, ClockPoint, Timestamp},
};

fn point(nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(19),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

fn assert_fraction(time: TargetTime, numerator: u128, denominator: u128) {
    assert_eq!(
        u128::from(time.seconds()) * denominator * u128::from(time.denominator())
            + u128::from(time.numerator()) * denominator,
        numerator * u128::from(time.denominator()),
        "exact duration must survive every epoch without flooring"
    );
}

#[test]
fn piecewise_duration_has_one_final_floor_with_nonzero_negative_origin() {
    const DEN: u128 = 14_112_000;
    let origin = point(-987_654_321);
    let mut time = TargetTime::from_frames(17, 44_100).unwrap();
    let mut ticks = 17 * (DEN / 44_100);
    for (frames, rate) in [(1, 48_000), (7, 32_000), (11, 44_100), (13, 48_000)] {
        time = time.checked_add_frames(frames, rate).unwrap();
        ticks += u128::from(frames) * (DEN / u128::from(rate));
        assert_fraction(time, ticks, DEN);
        let expected =
            i128::from(origin.timestamp.as_nanos()) + (ticks * 1_000_000_000 / DEN) as i128;
        assert_eq!(time.point(origin).unwrap(), point(expected as i64));
    }
    let mut partitioned = TargetTime::from_frames(0, 48_000).unwrap();
    for _ in 0..48_000 {
        partitioned = partitioned.checked_add_frames(1, 48_000).unwrap();
    }
    assert_eq!(partitioned, TargetTime::new(1, 0, 1).unwrap());
}

#[test]
fn reduction_carry_and_prime_rate_epochs_match_independent_rational_oracle() {
    let reduced = TargetTime::new(2, 6, 15).unwrap();
    assert_eq!(
        (
            reduced.seconds(),
            reduced.numerator(),
            reduced.denominator()
        ),
        (2, 2, 5)
    );
    let zero = TargetTime::new(0, 0, u64::MAX).unwrap();
    assert_eq!(
        (zero.seconds(), zero.numerator(), zero.denominator()),
        (0, 0, 1)
    );
    let mut duration = zero;
    let mut numerator = 0_u128;
    let mut denominator = 1_u128;
    for (frames, rate) in [(17, 44_099), (19, 47_999), (23, 31_999)] {
        numerator = numerator * u128::from(rate) + u128::from(frames) * denominator;
        denominator *= u128::from(rate);
        duration = duration.checked_add_frames(frames, rate).unwrap();
        assert_fraction(duration, numerator, denominator);
    }
    let carried = TargetTime::new(3, 2, 3)
        .unwrap()
        .checked_add_frames(1, 3)
        .unwrap();
    assert_eq!(carried, TargetTime::new(4, 0, 1).unwrap());
}

#[test]
fn invalid_rate_denominator_and_seconds_overflow_refuse_immutable_duration() {
    assert!(TargetTime::new(0, 0, 0).is_err());
    assert!(TargetTime::new(0, 3, 3).is_err());
    assert!(TargetTime::new(0, 4, 3).is_err());
    assert!(TargetTime::from_frames(1, 0).is_err());
    let full = TargetTime::new(u64::MAX, 0, 1).unwrap();
    assert!(full.checked_add_frames(1, 1).is_err());
    assert_eq!(full, TargetTime::new(u64::MAX, 0, 1).unwrap());
    let fractional = TargetTime::new(0, 1, u64::MAX).unwrap();
    assert!(fractional.checked_add_frames(1, 2).is_err());
    assert_eq!(
        fractional.checked_add_frames(0, 48_000).unwrap(),
        fractional
    );
    assert!(fractional.checked_add_frames(1, 0).is_err());
    assert_eq!(fractional.point(point(0)).unwrap(), point(0));
}

#[test]
fn timestamp_overflow_is_refused_after_exact_fractional_addition() {
    let fraction = TargetTime::new(0, 1, 3).unwrap();
    assert!(fraction.point(point(i64::MAX)).is_err());
    assert_eq!(
        fraction.point(point(i64::MIN)).unwrap(),
        point(i64::MIN + 333_333_333)
    );
    let huge = TargetTime::new(u64::MAX, 0, 1).unwrap();
    assert!(huge.point(point(i64::MIN)).is_err());
    assert_eq!(fraction, TargetTime::new(0, 1, 3).unwrap());
}
