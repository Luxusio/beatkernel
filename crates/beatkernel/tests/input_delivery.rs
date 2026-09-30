//! Explicit supplied-point delivery age fixtures; no clocks or grading invoked.
use beatkernel::{
    telemetry::{InputDeliveryError, InputDeliveryTelemetry, TimingSummary},
    time::{ClockDomainId, ClockPoint, Timestamp},
};

const DOMAIN: ClockDomainId = ClockDomainId(7);
fn point(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: DOMAIN,
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn age(observer: &mut InputDeliveryTelemetry, ns: i64) {
    let event = point(1000 - ns);
    let received = point(1000);
    let observation = observer.observe(event, received).unwrap();
    assert_eq!(observation.event, event);
    assert_eq!(observation.received, received);
    assert_eq!(observation.age_ns, ns as u64);
}

#[test]
fn literal_nearest_ranks_describe_supplied_ages_not_an_inferred_period() {
    let mut observer = InputDeliveryTelemetry::new(100, DOMAIN).unwrap();
    assert_eq!(observer.summary(), None);
    assert_eq!(observer.observed_events(), 0);
    // A permutation of 1..=100, with unchanged receipts and varying event times.
    for index in 0..100 {
        age(&mut observer, (index * 37) % 100 + 1);
    }
    assert_eq!(
        observer.summary(),
        Some(TimingSummary {
            samples: 100,
            p50_ns: 50,
            p95_ns: 95,
            p99_ns: 99,
            max_ns: 100
        })
    );
    assert_eq!(observer.observed_events(), 100);
}

#[test]
fn bounded_retained_tail_discards_expired_outliers_and_summary_is_read_only() {
    let mut observer = InputDeliveryTelemetry::new(4, DOMAIN).unwrap();
    for ns in [999, 100, 8, 2, 6, 4] {
        age(&mut observer, ns);
    }
    let retained = Some(TimingSummary {
        samples: 4,
        p50_ns: 4,
        p95_ns: 8,
        p99_ns: 8,
        max_ns: 8,
    });
    assert_eq!(observer.summary(), retained);
    assert_eq!(observer.summary(), retained);
    assert_eq!(observer.observed_events(), 6);
    assert_eq!(observer.capacity(), 4);
    age(&mut observer, 1);
    assert_eq!(
        observer.summary(),
        Some(TimingSummary {
            samples: 4,
            p50_ns: 2,
            p95_ns: 6,
            p99_ns: 6,
            max_ns: 6
        })
    );
    assert_eq!(observer.observed_events(), 7);
}

#[test]
fn disabled_retention_still_counts_and_validates_every_observation() {
    let mut observer = InputDeliveryTelemetry::new(0, DOMAIN).unwrap();
    assert_eq!(observer.capacity(), 0);
    assert_eq!(observer.domain(), DOMAIN);
    assert_eq!(observer.observe(point(0), point(1)).unwrap().age_ns, 1);
    assert_eq!(observer.observe(point(1), point(1)).unwrap().age_ns, 0);
    assert_eq!(observer.observed_events(), 2);
    assert_eq!(observer.summary(), None);
    assert!(matches!(
        observer.observe(point(2), point(1)),
        Err(InputDeliveryError::FutureEvent)
    ));
    assert!(matches!(
        observer.observe(point(0), point(0)),
        Err(InputDeliveryError::ReceivedTimeRegression)
    ));
    assert_eq!(observer.observed_events(), 2);
    assert_eq!(observer.summary(), None);
}

#[test]
fn zero_age_equal_receipts_and_full_signed_span_remain_representable() {
    let mut observer = InputDeliveryTelemetry::new(2, DOMAIN).unwrap();
    let observation = observer.observe(point(i64::MIN), point(i64::MAX)).unwrap();
    assert_eq!(observation.event, point(i64::MIN));
    assert_eq!(observation.received, point(i64::MAX));
    assert_eq!(observation.age_ns, u64::MAX);
    assert_eq!(
        observer.summary(),
        Some(TimingSummary {
            samples: 1,
            p50_ns: u64::MAX,
            p95_ns: u64::MAX,
            p99_ns: u64::MAX,
            max_ns: u64::MAX
        })
    );
    assert_eq!(
        observer
            .observe(point(i64::MAX), point(i64::MAX))
            .unwrap()
            .age_ns,
        0
    );
    assert_eq!(
        observer.summary(),
        Some(TimingSummary {
            samples: 2,
            p50_ns: 0,
            p95_ns: u64::MAX,
            p99_ns: u64::MAX,
            max_ns: u64::MAX
        })
    );
    assert_eq!(observer.observed_events(), 2);
}

#[test]
fn domain_future_and_receipt_regression_fail_before_count_ring_or_chronology_mutation() {
    let mut observer = InputDeliveryTelemetry::new(3, DOMAIN).unwrap();
    observer.observe(point(90), point(100)).unwrap();
    let before = observer.summary();
    let foreign_event = ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(1900),
    };
    assert!(matches!(
        observer.observe(foreign_event, point(2000)),
        Err(InputDeliveryError::ClockDomainMismatch {
            expected: DOMAIN,
            received: ClockDomainId(8)
        })
    ));
    assert_eq!(observer.summary(), before);
    assert_eq!(observer.observed_events(), 1);
    let foreign_receipt = ClockPoint {
        domain: ClockDomainId(9),
        timestamp: Timestamp::from_nanos(1000),
    };
    assert!(matches!(
        observer.observe(point(900), foreign_receipt),
        Err(InputDeliveryError::ClockDomainMismatch {
            expected: DOMAIN,
            received: ClockDomainId(9)
        })
    ));
    assert_eq!(observer.summary(), before);
    assert_eq!(observer.observed_events(), 1);
    assert!(matches!(
        observer.observe(point(2001), point(2000)),
        Err(InputDeliveryError::FutureEvent)
    ));
    assert_eq!(observer.summary(), before);
    assert_eq!(observer.observed_events(), 1);
    assert!(matches!(
        observer.observe(point(80), point(99)),
        Err(InputDeliveryError::ReceivedTimeRegression)
    ));
    assert_eq!(observer.summary(), before);
    assert_eq!(observer.observed_events(), 1);
    // Rejected future receipt points cannot advance the accepted receipt baseline.
    assert_eq!(observer.observe(point(70), point(100)).unwrap().age_ns, 30);
    assert_eq!(observer.observe(point(75), point(101)).unwrap().age_ns, 26);
    assert_eq!(
        observer.summary(),
        Some(TimingSummary {
            samples: 3,
            p50_ns: 26,
            p95_ns: 30,
            p99_ns: 30,
            max_ns: 30
        })
    );
    assert_eq!(observer.observed_events(), 3);
}

#[test]
fn cross_device_event_point_regressions_do_not_impose_judge_chronology() {
    let mut observer = InputDeliveryTelemetry::new(3, DOMAIN).unwrap();
    assert_eq!(observer.observe(point(90), point(100)).unwrap().age_ns, 10);
    assert_eq!(observer.observe(point(20), point(101)).unwrap().age_ns, 81);
    assert_eq!(observer.observe(point(99), point(102)).unwrap().age_ns, 3);
    assert_eq!(
        observer.summary(),
        Some(TimingSummary {
            samples: 3,
            p50_ns: 10,
            p95_ns: 81,
            p99_ns: 81,
            max_ns: 81
        })
    );
    assert_eq!(observer.observed_events(), 3);
}

#[test]
fn capacity_bounds_unknown_initial_state_and_single_zero_sample_are_explicit() {
    assert_eq!(InputDeliveryTelemetry::MAX_CAPACITY, 65_536);
    for capacity in [InputDeliveryTelemetry::MAX_CAPACITY + 1, usize::MAX] {
        assert!(matches!(
            InputDeliveryTelemetry::new(capacity, DOMAIN),
            Err(InputDeliveryError::InvalidCapacity)
        ));
    }
    let mut maximum =
        InputDeliveryTelemetry::new(InputDeliveryTelemetry::MAX_CAPACITY, DOMAIN).unwrap();
    assert_eq!(maximum.capacity(), 65_536);
    assert_eq!(maximum.domain(), DOMAIN);
    assert_eq!(maximum.summary(), None);
    assert_eq!(maximum.observed_events(), 0);
    maximum.observe(point(0), point(0)).unwrap();
    assert_eq!(
        maximum.summary(),
        Some(TimingSummary {
            samples: 1,
            p50_ns: 0,
            p95_ns: 0,
            p99_ns: 0,
            max_ns: 0
        })
    );
}

#[test]
fn clone_retains_history_and_receipt_baseline_without_sharing_mutation() {
    let mut original = InputDeliveryTelemetry::new(2, DOMAIN).unwrap();
    age(&mut original, 10);
    age(&mut original, 20);
    let mut clone = original.clone();
    age(&mut clone, 30);
    assert_eq!(
        original.summary(),
        Some(TimingSummary {
            samples: 2,
            p50_ns: 10,
            p95_ns: 20,
            p99_ns: 20,
            max_ns: 20
        })
    );
    assert_eq!(
        clone.summary(),
        Some(TimingSummary {
            samples: 2,
            p50_ns: 20,
            p95_ns: 30,
            p99_ns: 30,
            max_ns: 30
        })
    );
    assert_eq!(original.observed_events(), 2);
    assert_eq!(clone.observed_events(), 3);
    original.observe(point(900), point(1001)).unwrap();
    assert_eq!(clone.observe(point(999), point(1000)).unwrap().age_ns, 1);
    assert_eq!(
        original.summary(),
        Some(TimingSummary {
            samples: 2,
            p50_ns: 20,
            p95_ns: 101,
            p99_ns: 101,
            max_ns: 101
        })
    );
    assert_eq!(
        clone.summary(),
        Some(TimingSummary {
            samples: 2,
            p50_ns: 1,
            p95_ns: 30,
            p99_ns: 30,
            max_ns: 30
        })
    );
    assert_eq!(original.observed_events(), 3);
    assert_eq!(clone.observed_events(), 4);
}
