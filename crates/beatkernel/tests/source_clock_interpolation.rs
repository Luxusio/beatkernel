//! Original signed clock anchors retain exact fractions and one final floor.
use beatkernel::{
    audio::{AudioError, ConvertedPhaseError, InterpolatedTargetTime, TargetTime},
    time::{ClockDomainId, ClockPoint, Timestamp},
};
use std::cmp::Ordering;
fn zero() -> TargetTime {
    TargetTime::new(0, 0, 1).unwrap()
}
fn origin(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::from_nanos(ns),
    }
}

#[test]
fn actual_prearm_bracket_first_integer_host_is_1536224814_without_zero_clamping() {
    for (host, order, nanos) in [
        (1_530_920_000, Ordering::Less, -5_303_245),
        (1_536_224_813, Ordering::Less, -1),
        (1_536_224_814, Ordering::Greater, 0),
        (1_537_685_000, Ordering::Greater, 1_459_755),
    ] {
        let q = InterpolatedTargetTime::from_signed_native(
            zero(),
            -5_303_245,
            1_459_755,
            1_000_000_000,
            i128::from(host - 1_530_920_000),
            6_765_000,
        )
        .unwrap();
        assert_eq!(q.compare_target_time(zero()), order);
        assert_eq!(q.compare_source_time_frame(0, 44_100), Ok(order));
        assert_eq!(q.point(origin(0)).unwrap(), origin(nanos));
    }
}

#[test]
fn signed_fractional_base_and_extrapolation_have_one_final_floor() {
    let base = TargetTime::new(0, 1, 3).unwrap();
    for (offset, fraction, nanos) in [
        (-2, 2, 19_047_619),
        (2, 38, 361_904_761),
        (7, 83, 790_476_190),
    ] {
        let q = InterpolatedTargetTime::from_signed_native(base, -1, 2, 7, offset, 5).unwrap();
        assert_eq!(
            q.compare_target_time(TargetTime::new(0, fraction, 105).unwrap()),
            Ordering::Equal
        );
        assert_eq!(q.point(origin(0)).unwrap(), origin(nanos));
    }
    let negative = InterpolatedTargetTime::from_signed_native(zero(), -1, 0, 3, 1, 2).unwrap();
    assert_eq!(negative.point(origin(0)).unwrap(), origin(-166_666_667));
    assert_eq!(
        negative.elapsed_nanos_since_source_frame(0, 1),
        Err(ConvertedPhaseError::NegativeElapsed)
    );
}

#[test]
fn full_signed_anchor_span_keeps_the_negative_half_tick() {
    for (offset, nanos, order) in [
        (0, -500_000_001, Ordering::Less),
        (1, -1, Ordering::Less),
        (2, 499_999_999, Ordering::Greater),
    ] {
        let q = InterpolatedTargetTime::from_signed_native(
            zero(),
            i64::MIN,
            i64::MAX,
            u64::MAX,
            offset,
            2,
        )
        .unwrap();
        assert_eq!(q.point(origin(0)).unwrap(), origin(nanos));
        assert_eq!(q.compare_target_time(zero()), order);
    }
}

#[test]
fn huge_host_products_cancel_to_exact_signed_endpoints_and_midpoint() {
    for (offset, expected) in [
        (0, i64::MIN),
        (1i128 << 63, 0),
        (i128::from(u64::MAX), i64::MAX),
    ] {
        let q = InterpolatedTargetTime::from_signed_native(
            zero(),
            i64::MIN,
            i64::MAX,
            1_000_000_000,
            offset,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(q.point(origin(0)).unwrap(), origin(expected));
    }
}

#[test]
fn negative_anchor_cancels_large_base_before_elapsed_narrowing() {
    let q = InterpolatedTargetTime::from_signed_native(
        TargetTime::new(u64::MAX, 0, 1).unwrap(),
        -1,
        -1,
        1,
        0,
        1,
    )
    .unwrap();
    assert_eq!(
        q.compare_source_time_frame(u64::MAX - 1, 1),
        Ok(Ordering::Equal)
    );
    assert_eq!(q.elapsed_nanos_since_source_frame(u64::MAX - 1, 1), Ok(0));
    assert_eq!(q.point(origin(0)), Err(AudioError::Overflow));
}

#[test]
fn signed_constructor_rejects_invalid_units_regression_and_unrepresentable_offsets() {
    for (left, right, frequency, offset, span, expected) in [
        (-1, 1, 0, 0, 1, AudioError::InvalidFormat),
        (-1, 1, 1, 0, 0, AudioError::InvalidFormat),
        (1, -1, 1, 0, 1, AudioError::InvalidFormat),
        (-1, 1, 1, i128::MIN, 1, AudioError::Overflow),
        (-1, 1, 1, i128::from(u64::MAX) + 1, 1, AudioError::Overflow),
        (-1, 1, 1, -i128::from(u64::MAX) - 1, 1, AudioError::Overflow),
    ] {
        assert!(
            matches!(InterpolatedTargetTime::from_signed_native(zero(), left, right,
            frequency, offset, span), Err(actual) if actual == expected)
        );
    }
    assert!(matches!(
        InterpolatedTargetTime::from_signed_native(
            zero(),
            i64::MIN,
            i64::MAX,
            1,
            i128::from(u64::MAX),
            1
        ),
        Err(AudioError::Overflow)
    ));
}

#[test]
fn existing_unsigned_constructor_retains_its_full_range_and_exact_semantics() {
    for (left, right, frequency, offset, span) in [
        (0, 1, 3, -1, 2),
        (10, 20, 3, -2, 7),
        (10, 20, 3, 9, 7),
        (i64::MAX, i64::MAX, u64::MAX, 1, u64::MAX),
    ] {
        let unsigned =
            InterpolatedTargetTime::new(zero(), left as u64, right as u64, frequency, offset, span)
                .unwrap();
        let signed = InterpolatedTargetTime::from_signed_native(
            zero(),
            left,
            right,
            frequency,
            offset,
            span,
        )
        .unwrap();
        assert_eq!(unsigned.compare_interpolated_time(signed), Ordering::Equal);
    }
    let full = InterpolatedTargetTime::new(zero(), u64::MAX, u64::MAX, 1, 0, 1).unwrap();
    assert_eq!(
        full.compare_target_time(TargetTime::new(u64::MAX, 0, 1).unwrap()),
        Ordering::Equal
    );
}
