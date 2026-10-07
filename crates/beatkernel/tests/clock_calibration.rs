use beatkernel::time::*;
fn ts(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}
fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: ts(nanos),
    }
}
fn pair(source: i64, target: i64) -> ClockPair {
    ClockPair {
        source: point(1, source),
        target: point(2, target),
    }
}
fn interval(start: i64, end: i64) -> ClockInterval {
    ClockInterval {
        start: ts(start),
        end: ts(end),
    }
}
fn error(observation: i64, drift: Option<i64>) -> CalibrationUncertainty {
    CalibrationUncertainty {
        observation_error: Duration::from_nanos(observation),
        residual_drift_error: drift.map(Duration::from_nanos),
    }
}
#[test]
fn preserved_anchors_and_exact_integer_affine_forward_inverse() {
    let first = pair(100, 1000);
    let second = pair(200, 1200);
    let mapper = AffineClockMapper::from_pairs(
        first,
        second,
        interval(100, 200),
        ExtrapolationPolicy::Forbid,
        error(2, Some(3)),
    )
    .unwrap();
    assert_eq!(mapper.anchors(), (first, Some(second)));
    assert_eq!(mapper.rate(), (2, 1));
    assert_eq!(mapper.source_interval(), interval(100, 200));
    assert_eq!(mapper.target_interval(), interval(1000, 1200));
    assert_eq!(
        mapper.map_checked(first.source, ClockDomainId(2)).unwrap(),
        first.target.timestamp
    );
    assert_eq!(
        mapper.map_checked(second.target, ClockDomainId(1)).unwrap(),
        second.source.timestamp
    );
    assert_eq!(
        mapper.map_checked(point(1, 125), ClockDomainId(2)).unwrap(),
        ts(1050)
    );
    assert_eq!(
        mapper
            .map_checked(point(2, 1050), ClockDomainId(1))
            .unwrap(),
        ts(125)
    );
    assert_eq!(
        mapper.quality(),
        ClockMappingQuality::Estimated {
            max_error: Duration::from_nanos(6)
        }
    );
}
#[test]
fn observed_pairs_never_invent_exactness_or_drift_guarantees() {
    let mapper = AffineClockMapper::from_pairs(
        pair(0, 500),
        pair(100, 600),
        interval(0, 100),
        ExtrapolationPolicy::Forbid,
        error(0, None),
    )
    .unwrap();
    assert_eq!(mapper.quality(), ClockMappingQuality::Unknown);
    let estimated = AffineClockMapper::from_pairs(
        pair(0, 500),
        pair(100, 600),
        interval(0, 100),
        ExtrapolationPolicy::Forbid,
        error(0, Some(0)),
    )
    .unwrap();
    assert_eq!(
        estimated.quality(),
        ClockMappingQuality::Estimated {
            max_error: Duration::ZERO
        }
    );
    let exact = AffineClockMapper::exact_offset(pair(0, 500), interval(-10, 10)).unwrap();
    assert_eq!(exact.quality(), ClockMappingQuality::Exact);
    assert_eq!(exact.anchors(), (pair(0, 500), None));
    for nanos in -10..=10 {
        let mapped = exact
            .map_checked(point(1, nanos), ClockDomainId(2))
            .unwrap();
        assert_eq!(
            exact
                .map_checked(
                    ClockPoint {
                        domain: ClockDomainId(2),
                        timestamp: mapped
                    },
                    ClockDomainId(1)
                )
                .unwrap(),
            ts(nanos)
        );
    }
}
#[test]
fn extrapolation_and_validity_are_explicit_finite_in_both_domains() {
    assert_eq!(
        AffineClockMapper::from_pairs(
            pair(0, 10),
            pair(1000, 510),
            interval(-100, 1100),
            ExtrapolationPolicy::Forbid,
            error(1, None)
        )
        .unwrap_err(),
        CalibrationError::ValidityOutsideEnvelope
    );
    let mapper = AffineClockMapper::from_pairs(
        pair(0, 10),
        pair(1000, 510),
        interval(-100, 1100),
        ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(100),
            after: Duration::from_nanos(100),
        },
        error(1, None),
    )
    .unwrap();
    assert_eq!(
        mapper
            .map_checked(point(1, -100), ClockDomainId(2))
            .unwrap(),
        ts(-40)
    );
    assert_eq!(
        mapper
            .map_checked(point(1, 1100), ClockDomainId(2))
            .unwrap(),
        ts(560)
    );
    assert_eq!(
        mapper.map_checked(point(1, 1101), ClockDomainId(2)),
        Err(CalibrationError::OutsideValidity {
            domain: ClockDomainId(1)
        })
    );
    assert_eq!(
        mapper.map_checked(point(2, 561), ClockDomainId(1)),
        Err(CalibrationError::OutsideValidity {
            domain: ClockDomainId(2)
        })
    );
    assert_eq!(
        mapper.map_checked(point(1, -101), ClockDomainId(1)),
        Err(CalibrationError::OutsideValidity {
            domain: ClockDomainId(1)
        })
    );
    assert_eq!(mapper.map(point(1, 1101), ClockDomainId(2)), None);
    assert!(matches!(
        mapper.map_checked(point(3, 0), ClockDomainId(3)),
        Err(CalibrationError::DomainMismatch { .. })
    ));
}
#[test]
fn rounded_inverse_boundaries_clamp_and_roundtrip_error_is_bounded() {
    let mapper = AffineClockMapper::from_pairs(
        pair(0, 0),
        pair(1000, 1),
        interval(100, 200),
        ExtrapolationPolicy::Forbid,
        error(0, Some(0)),
    )
    .unwrap();
    assert_eq!(mapper.target_interval(), interval(0, 0));
    assert_eq!(
        mapper.map_checked(point(1, 200), ClockDomainId(2)).unwrap(),
        ts(0)
    );
    assert_eq!(
        mapper.map_checked(point(2, 0), ClockDomainId(1)).unwrap(),
        ts(100)
    );
    assert_eq!(
        mapper.quality(),
        ClockMappingQuality::Estimated {
            max_error: Duration::from_nanos(1001)
        }
    );
    let mapper = AffineClockMapper::from_pairs(
        pair(-100, 37),
        pair(100, 171),
        interval(-100, 100),
        ExtrapolationPolicy::Forbid,
        error(1, Some(2)),
    )
    .unwrap();
    let (numerator, denominator) = mapper.rate();
    let bound = denominator.div_ceil(numerator) + 1;
    for value in -100..=100 {
        let forward = mapper
            .map_checked(point(1, value), ClockDomainId(2))
            .unwrap();
        let inverse = mapper
            .map_checked(
                ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: forward,
                },
                ClockDomainId(1),
            )
            .unwrap();
        assert!(inverse.as_nanos().abs_diff(value) <= bound);
    }
}
#[test]
fn wide_products_map_representable_extreme_anchors_and_interiors() {
    let mapper = AffineClockMapper::from_pairs(
        pair(i64::MIN, i64::MIN),
        pair(i64::MAX, i64::MAX - 1),
        interval(i64::MIN, i64::MAX),
        ExtrapolationPolicy::Forbid,
        error(0, None),
    )
    .unwrap();
    assert_eq!(
        mapper
            .map_checked(point(1, i64::MAX), ClockDomainId(2))
            .unwrap(),
        ts(i64::MAX - 1)
    );
    assert_eq!(
        mapper
            .map_checked(point(1, i64::MAX - 1), ClockDomainId(2))
            .unwrap(),
        ts(i64::MAX - 2)
    );
    assert_eq!(
        mapper
            .map_checked(point(2, i64::MAX - 1), ClockDomainId(1))
            .unwrap(),
        ts(i64::MAX)
    );
    assert_eq!(
        AffineClockMapper::exact_offset(pair(i64::MIN, i64::MAX), interval(i64::MIN, i64::MIN + 1))
            .unwrap_err(),
        CalibrationError::Overflow
    );
    let unknown_bound = AffineClockMapper::from_pairs(
        pair(0, 0),
        pair(i64::MAX, 1),
        interval(0, i64::MAX),
        ExtrapolationPolicy::Forbid,
        error(i64::MAX, Some(i64::MAX)),
    )
    .unwrap();
    assert_eq!(unknown_bound.quality(), ClockMappingQuality::Unknown);
}
#[test]
fn malformed_domains_ranges_and_estimates_fail_without_a_mapper() {
    let first = pair(0, 100);
    let second = pair(10, 110);
    assert_eq!(
        AffineClockMapper::from_pairs(
            first,
            pair(0, 110),
            interval(0, 10),
            ExtrapolationPolicy::Forbid,
            error(0, None)
        )
        .unwrap_err(),
        CalibrationError::NonIncreasingAnchors
    );
    assert_eq!(
        AffineClockMapper::from_pairs(
            first,
            pair(10, 90),
            interval(0, 10),
            ExtrapolationPolicy::Forbid,
            error(0, None)
        )
        .unwrap_err(),
        CalibrationError::NonIncreasingAnchors
    );
    let mismatched = ClockPair {
        source: point(3, 10),
        target: point(2, 110),
    };
    assert_eq!(
        AffineClockMapper::from_pairs(
            first,
            mismatched,
            interval(0, 10),
            ExtrapolationPolicy::Forbid,
            error(0, None)
        )
        .unwrap_err(),
        CalibrationError::PairDomainMismatch
    );
    assert_eq!(
        AffineClockMapper::from_pairs(
            first,
            second,
            interval(10, 0),
            ExtrapolationPolicy::Forbid,
            error(0, None)
        )
        .unwrap_err(),
        CalibrationError::InvalidInterval
    );
    assert_eq!(
        AffineClockMapper::from_pairs(
            first,
            second,
            interval(0, 10),
            ExtrapolationPolicy::Forbid,
            error(-1, None)
        )
        .unwrap_err(),
        CalibrationError::NegativeUncertainty
    );
    assert_eq!(
        AffineClockMapper::from_pairs(
            first,
            second,
            interval(0, 10),
            ExtrapolationPolicy::Bounded {
                before: Duration::from_nanos(-1),
                after: Duration::ZERO
            },
            error(0, None)
        )
        .unwrap_err(),
        CalibrationError::NegativeExtrapolation
    );
    let identical = ClockPair {
        source: point(1, 0),
        target: point(1, 100),
    };
    assert_eq!(
        AffineClockMapper::exact_offset(identical, interval(0, 10)).unwrap_err(),
        CalibrationError::IdenticalDomains
    );
}

#[test]
fn unknown_observations_map_unequal_rates_analytically_without_assigning_uncertainty() {
    // The target advances 300 ns while the source advances 200 ns: 3/2.
    let first = pair(100, 1000);
    let second = pair(300, 1300);
    let mapper = AffineClockMapper::from_pairs_unknown(
        first,
        second,
        interval(100, 300),
        ExtrapolationPolicy::Forbid,
    )
    .unwrap();
    assert_eq!(mapper.anchors(), (first, Some(second)));
    assert_eq!(mapper.rate(), (3, 2));
    assert_eq!(mapper.source_interval(), interval(100, 300));
    assert_eq!(mapper.target_interval(), interval(1000, 1300));
    assert_eq!(mapper.uncertainty(), None);
    assert_eq!(mapper.quality(), ClockMappingQuality::Unknown);
    for (source, target) in [
        (100, 1000),
        (150, 1075),
        (200, 1150),
        (250, 1225),
        (300, 1300),
    ] {
        assert_eq!(
            mapper
                .map_checked(point(1, source), ClockDomainId(2))
                .unwrap(),
            ts(target)
        );
        assert_eq!(
            mapper
                .map_checked(point(2, target), ClockDomainId(1))
                .unwrap(),
            ts(source)
        );
        assert_eq!(
            mapper.map(point(1, source), ClockDomainId(2)),
            Some(ts(target))
        );
    }
    // A unit-rate fit also supplies no observed numerical error guarantee.
    let equal_rate = AffineClockMapper::from_pairs_unknown(
        pair(-100, 900),
        pair(100, 1100),
        interval(-100, 100),
        ExtrapolationPolicy::Forbid,
    )
    .unwrap();
    assert_eq!(equal_rate.uncertainty(), None);
    assert_eq!(equal_rate.quality(), ClockMappingQuality::Unknown);
    assert_eq!(
        equal_rate
            .map_checked(point(1, 0), ClockDomainId(2))
            .unwrap(),
        ts(1000)
    );
}

#[test]
fn unknown_observation_validity_is_inclusive_finite_in_both_domains() {
    let mapper = AffineClockMapper::from_pairs_unknown(
        pair(100, 1000),
        pair(300, 1300),
        interval(150, 250),
        ExtrapolationPolicy::Forbid,
    )
    .unwrap();
    assert_eq!(mapper.source_interval(), interval(150, 250));
    assert_eq!(mapper.target_interval(), interval(1075, 1225));
    for (source, target) in [(150, 1075), (250, 1225)] {
        assert_eq!(
            mapper.map_checked(point(1, source), ClockDomainId(2)),
            Ok(ts(target))
        );
        assert_eq!(
            mapper.map_checked(point(2, target), ClockDomainId(1)),
            Ok(ts(source))
        );
    }
    for source in [149, 251] {
        assert_eq!(
            mapper.map_checked(point(1, source), ClockDomainId(2)),
            Err(CalibrationError::OutsideValidity {
                domain: ClockDomainId(1)
            })
        );
        assert_eq!(
            mapper.map_checked(point(1, source), ClockDomainId(1)),
            Err(CalibrationError::OutsideValidity {
                domain: ClockDomainId(1)
            })
        );
        assert_eq!(mapper.map(point(1, source), ClockDomainId(2)), None);
    }
    for target in [1074, 1226] {
        assert_eq!(
            mapper.map_checked(point(2, target), ClockDomainId(1)),
            Err(CalibrationError::OutsideValidity {
                domain: ClockDomainId(2)
            })
        );
        assert_eq!(mapper.map(point(2, target), ClockDomainId(1)), None);
    }
    for (from, to) in [(3, 1), (1, 3), (3, 3)] {
        assert_eq!(
            mapper.map_checked(point(from, 200), ClockDomainId(to)),
            Err(CalibrationError::DomainMismatch {
                from: ClockDomainId(from),
                to: ClockDomainId(to)
            })
        );
    }
}

#[test]
fn unknown_observation_extrapolation_requires_explicit_nonnegative_finite_limits() {
    let bounded = ExtrapolationPolicy::Bounded {
        before: Duration::from_nanos(20),
        after: Duration::from_nanos(40),
    };
    let mapper = AffineClockMapper::from_pairs_unknown(
        pair(100, 1000),
        pair(300, 1300),
        interval(80, 340),
        bounded,
    )
    .unwrap();
    assert_eq!(mapper.quality(), ClockMappingQuality::Unknown);
    assert_eq!(mapper.uncertainty(), None);
    assert_eq!(mapper.target_interval(), interval(970, 1360));
    for (source, target) in [(80, 970), (340, 1360)] {
        assert_eq!(
            mapper.map_checked(point(1, source), ClockDomainId(2)),
            Ok(ts(target))
        );
        assert_eq!(
            mapper.map_checked(point(2, target), ClockDomainId(1)),
            Ok(ts(source))
        );
    }
    for validity in [interval(79, 300), interval(100, 341)] {
        assert_eq!(
            AffineClockMapper::from_pairs_unknown(
                pair(100, 1000),
                pair(300, 1300),
                validity,
                bounded
            )
            .unwrap_err(),
            CalibrationError::ValidityOutsideEnvelope
        );
    }
    for validity in [interval(99, 300), interval(100, 301)] {
        assert_eq!(
            AffineClockMapper::from_pairs_unknown(
                pair(100, 1000),
                pair(300, 1300),
                validity,
                ExtrapolationPolicy::Forbid
            )
            .unwrap_err(),
            CalibrationError::ValidityOutsideEnvelope
        );
    }
    for (before, after) in [(-1, 0), (0, -1)] {
        assert_eq!(
            AffineClockMapper::from_pairs_unknown(
                pair(100, 1000),
                pair(300, 1300),
                interval(100, 300),
                ExtrapolationPolicy::Bounded {
                    before: Duration::from_nanos(before),
                    after: Duration::from_nanos(after)
                }
            )
            .unwrap_err(),
            CalibrationError::NegativeExtrapolation
        );
    }
    assert_eq!(
        mapper.map_checked(point(1, 341), ClockDomainId(2)),
        Err(CalibrationError::OutsideValidity {
            domain: ClockDomainId(1)
        })
    );
    assert_eq!(
        mapper.map_checked(point(2, 1361), ClockDomainId(1)),
        Err(CalibrationError::OutsideValidity {
            domain: ClockDomainId(2)
        })
    );
}

#[test]
fn unknown_observation_constructor_rejects_domains_anchor_order_and_reversed_validity() {
    let first = pair(100, 1000);
    for second in [
        pair(100, 1300),
        pair(99, 1300),
        pair(300, 1000),
        pair(300, 999),
    ] {
        assert_eq!(
            AffineClockMapper::from_pairs_unknown(
                first,
                second,
                interval(100, 300),
                ExtrapolationPolicy::Forbid
            )
            .unwrap_err(),
            CalibrationError::NonIncreasingAnchors
        );
    }
    for second in [
        ClockPair {
            source: point(3, 300),
            target: point(2, 1300),
        },
        ClockPair {
            source: point(1, 300),
            target: point(3, 1300),
        },
    ] {
        assert_eq!(
            AffineClockMapper::from_pairs_unknown(
                first,
                second,
                interval(100, 300),
                ExtrapolationPolicy::Forbid
            )
            .unwrap_err(),
            CalibrationError::PairDomainMismatch
        );
    }
    assert_eq!(
        AffineClockMapper::from_pairs_unknown(
            ClockPair {
                source: point(1, 100),
                target: point(1, 1000)
            },
            ClockPair {
                source: point(1, 300),
                target: point(1, 1300)
            },
            interval(100, 300),
            ExtrapolationPolicy::Forbid,
        )
        .unwrap_err(),
        CalibrationError::IdenticalDomains
    );
    assert_eq!(
        AffineClockMapper::from_pairs_unknown(
            first,
            pair(300, 1300),
            interval(250, 150),
            ExtrapolationPolicy::Forbid
        )
        .unwrap_err(),
        CalibrationError::InvalidInterval
    );
}

#[test]
fn unknown_observations_preserve_wide_arithmetic_and_refuse_unrepresentable_targets() {
    let mapper = AffineClockMapper::from_pairs_unknown(
        pair(i64::MIN, i64::MIN),
        pair(i64::MAX, i64::MAX - 1),
        interval(i64::MIN, i64::MAX),
        ExtrapolationPolicy::Forbid,
    )
    .unwrap();
    assert_eq!(
        mapper.map_checked(point(1, i64::MIN), ClockDomainId(2)),
        Ok(ts(i64::MIN))
    );
    assert_eq!(
        mapper.map_checked(point(1, i64::MAX), ClockDomainId(2)),
        Ok(ts(i64::MAX - 1))
    );
    assert_eq!(
        mapper.map_checked(point(1, i64::MAX - 1), ClockDomainId(2)),
        Ok(ts(i64::MAX - 2))
    );
    assert_eq!(
        mapper.map_checked(point(2, i64::MAX - 1), ClockDomainId(1)),
        Ok(ts(i64::MAX))
    );
    assert_eq!(mapper.uncertainty(), None);
    assert_eq!(mapper.quality(), ClockMappingQuality::Unknown);
    assert_eq!(
        AffineClockMapper::from_pairs_unknown(
            pair(0, i64::MAX - 1),
            pair(1, i64::MAX),
            interval(0, 2),
            ExtrapolationPolicy::Bounded {
                before: Duration::ZERO,
                after: Duration::from_nanos(1)
            }
        )
        .unwrap_err(),
        CalibrationError::Overflow
    );
    assert_eq!(
        AffineClockMapper::from_pairs_unknown(
            pair(0, i64::MIN),
            pair(1, i64::MIN + 1),
            interval(-1, 1),
            ExtrapolationPolicy::Bounded {
                before: Duration::from_nanos(1),
                after: Duration::ZERO
            }
        )
        .unwrap_err(),
        CalibrationError::Overflow
    );
}
