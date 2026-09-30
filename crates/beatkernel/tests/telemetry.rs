use beatkernel::{telemetry::*, time::*};

fn point(domain: u32, nanos: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(nanos),
    }
}

#[test]
fn explicit_baseline_is_not_a_sample_and_invalid_capacity_is_rejected() {
    let jitter = IntervalJitter::new(4, Duration::from_nanos(10), point(1, 100)).unwrap();
    assert_eq!(jitter.summary().unwrap(), None);
    assert_eq!(jitter.observed_pairs(), 0);
    assert_eq!(jitter.baseline(), point(1, 100));
    assert_eq!(jitter.nominal_interval(), Duration::from_nanos(10));
    assert_eq!(jitter.capacity(), 4);
    assert!(matches!(
        IntervalJitter::new(0, Duration::from_nanos(10), point(1, 100)),
        Err(IntervalJitterError::InvalidCapacity)
    ));
    assert!(matches!(
        IntervalJitter::new(
            IntervalJitter::MAX_CAPACITY + 1,
            Duration::from_nanos(10),
            point(1, 100)
        ),
        Err(IntervalJitterError::InvalidCapacity)
    ));
    for nominal in [Duration::ZERO, Duration::from_nanos(-1), Duration::MIN] {
        assert!(matches!(
            IntervalJitter::new(4, nominal, point(1, 100)),
            Err(IntervalJitterError::InvalidNominalInterval)
        ));
    }
}

#[test]
fn equal_points_preserve_signed_early_deviation_and_absolute_percentiles() {
    let mut jitter = IntervalJitter::new(4, Duration::from_nanos(10), point(1, 100)).unwrap();
    let early = jitter.observe(point(1, 105)).unwrap();
    assert_eq!((early.elapsed_ns, early.deviation_ns), (5, -5));
    let equal = jitter.observe(point(1, 105)).unwrap();
    assert_eq!((equal.elapsed_ns, equal.deviation_ns), (0, -10));
    jitter.observe(point(1, 115)).unwrap();
    jitter.observe(point(1, 135)).unwrap();
    assert_eq!(
        jitter.summary().unwrap(),
        Some(IntervalJitterSummary {
            samples: 4,
            min_deviation_ns: -10,
            max_deviation_ns: 10,
            p50_abs_deviation_ns: 5,
            p95_abs_deviation_ns: 10,
            p99_abs_deviation_ns: 10,
            max_abs_deviation_ns: 10
        })
    );
}

#[test]
fn expired_outliers_leave_tail_extrema_and_percentiles_but_not_pair_count() {
    let mut jitter = IntervalJitter::new(3, Duration::from_nanos(10), point(1, 0)).unwrap();
    for nanos in [110, 120, 128, 141] {
        jitter.observe(point(1, nanos)).unwrap();
    }
    assert_eq!(
        jitter.summary().unwrap(),
        Some(IntervalJitterSummary {
            samples: 3,
            min_deviation_ns: -2,
            max_deviation_ns: 3,
            p50_abs_deviation_ns: 2,
            p95_abs_deviation_ns: 3,
            p99_abs_deviation_ns: 3,
            max_abs_deviation_ns: 3
        })
    );
    assert_eq!(jitter.observed_pairs(), 4);
    let mut clone = jitter.clone();
    clone.observe(point(1, 241)).unwrap();
    assert_eq!(jitter.baseline(), point(1, 141));
    assert_eq!(jitter.summary().unwrap().unwrap().max_abs_deviation_ns, 3);
    assert_eq!(clone.summary().unwrap().unwrap().max_abs_deviation_ns, 90);
}

#[test]
fn rejection_and_invalid_reset_are_atomic_and_do_not_change_next_interval() {
    let mut jitter = IntervalJitter::new(8, Duration::from_nanos(10), point(1, 100)).unwrap();
    jitter.observe(point(1, 110)).unwrap();
    let before = jitter.summary().unwrap();
    assert_eq!(
        jitter.observe(point(2, 120)),
        Err(IntervalJitterError::ClockDomainMismatch {
            expected: ClockDomainId(1),
            received: ClockDomainId(2)
        })
    );
    assert_eq!(
        jitter.observe(point(1, 109)),
        Err(IntervalJitterError::TimestampRegression)
    );
    assert_eq!(
        jitter.reset(Duration::ZERO, point(2, -100)),
        Err(IntervalJitterError::InvalidNominalInterval)
    );
    assert_eq!(
        jitter.reset(Duration::MIN, point(2, -100)),
        Err(IntervalJitterError::InvalidNominalInterval)
    );
    assert_eq!(jitter.summary().unwrap(), before);
    assert_eq!(jitter.baseline(), point(1, 110));
    assert_eq!(jitter.nominal_interval(), Duration::from_nanos(10));
    assert_eq!(jitter.observed_pairs(), 1);
    assert_eq!(jitter.observe(point(1, 120)).unwrap().deviation_ns, 0);
}

#[test]
fn explicit_reset_allows_domain_and_epoch_change_and_clears_pair_history() {
    let mut jitter = IntervalJitter::new(2, Duration::from_nanos(10), point(1, 100)).unwrap();
    jitter.observe(point(1, 130)).unwrap();
    jitter
        .reset(Duration::from_nanos(5), point(2, -100))
        .unwrap();
    assert_eq!(jitter.summary().unwrap(), None);
    assert_eq!(jitter.observed_pairs(), 0);
    assert_eq!(jitter.capacity(), 2);
    assert_eq!(jitter.observe(point(2, -95)).unwrap().deviation_ns, 0);
    assert_eq!(jitter.observe(point(2, -95)).unwrap().deviation_ns, -5);
}

#[test]
fn full_signed_timestamp_span_and_max_nominal_use_wide_arithmetic() {
    let mut jitter = IntervalJitter::new(
        2,
        Duration::from_nanos(1),
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::MIN,
        },
    )
    .unwrap();
    let observation = jitter
        .observe(ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::MAX,
        })
        .unwrap();
    assert_eq!(observation.elapsed_ns, u64::MAX);
    assert_eq!(observation.deviation_ns, i128::from(u64::MAX) - 1);
    assert_eq!(
        jitter.summary().unwrap().unwrap().max_abs_deviation_ns,
        u64::MAX - 1
    );
    jitter
        .reset(
            Duration::MAX,
            ClockPoint {
                domain: ClockDomainId(7),
                timestamp: Timestamp::MIN,
            },
        )
        .unwrap();
    assert_eq!(
        jitter
            .observe(ClockPoint {
                domain: ClockDomainId(7),
                timestamp: Timestamp::MIN
            })
            .unwrap()
            .deviation_ns,
        -i128::from(i64::MAX)
    );
    let span = jitter
        .observe(ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::MAX,
        })
        .unwrap();
    assert_eq!(span.elapsed_ns, u64::MAX);
    assert_eq!(span.deviation_ns, i128::from(i64::MAX) + 1);
}

#[test]
fn nearest_rank_percentiles_and_original_runtime_telemetry_remain_compatible() {
    let mut jitter = IntervalJitter::new(100, Duration::from_nanos(1), point(1, 0)).unwrap();
    let mut at = 0;
    for deviation in 1..=100 {
        at += deviation + 1;
        jitter.observe(point(1, at)).unwrap();
    }
    let summary = jitter.summary().unwrap().unwrap();
    assert_eq!(
        (
            summary.p50_abs_deviation_ns,
            summary.p95_abs_deviation_ns,
            summary.p99_abs_deviation_ns,
            summary.max_abs_deviation_ns
        ),
        (50, 95, 99, 100)
    );
    let mut software = RuntimeTelemetry::new(0);
    software.record_processing_ns(10);
    software.report_input_drops(3);
    software.report_audio_underruns(4);
    assert_eq!(software.processing(), None);
    assert_eq!(software.counters().input_drops, 3);
    assert_eq!(software.counters().audio_underruns, 4);
    let mut retained = RuntimeTelemetry::new(2);
    for duration in [100, 10, 20] {
        retained.record_processing_ns(duration);
    }
    assert_eq!(
        retained.processing(),
        Some(TimingSummary {
            samples: 2,
            p50_ns: 10,
            p95_ns: 20,
            p99_ns: 20,
            max_ns: 20
        })
    );
}
