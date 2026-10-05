//! Deferred pure-clock-pair contracts; no platform, clock reads or native IO.
use beatkernel::{
    time::{
        presentation::{
            DisciplineConfig, DisciplineUpdate, EstimatorError, ObservationAdmission,
            PresentationEstimator,
        },
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
    },
    transport::{Rate, Transport, TransportError},
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn pair(output: i64, host: i64) -> ClockPair {
    ClockPair {
        source: point(2, output),
        target: point(1, host),
    }
}
fn estimator(config: DisciplineConfig) -> PresentationEstimator {
    PresentationEstimator::new(config, point(2, 0), ClockDomainId(1), Timestamp::ZERO).unwrap()
}
fn normal(song: i64) -> Transport {
    Transport::new(Timestamp::ZERO, Timestamp::from_nanos(song), Rate::NORMAL)
}
fn applied(base: i64, correction: i64, rate: i64, phase: i128, limited: bool) -> DisciplineUpdate {
    DisciplineUpdate::Applied {
        base_rate_ppm: base,
        correction_ppm: correction,
        applied_rate_ppm: rate,
        phase_error_ns: phase,
        limited,
    }
}

#[test]
fn rational_drift_signed_phase_rounding_and_clamping_preserve_historical_transport() {
    // One second of output differs by +/-250 us: +/-250 ppm. A 500 us
    // phase over ten seconds adds +/-50 ppm. The other rows isolate clamp
    // and half-unit phase rounding with literal independent expected values.
    for (first, last, song, horizon, expected, future) in [
        (
            1_000_250_000,
            2_000_500_000,
            0,
            10_000_000_000,
            applied(250, 50, 300, 500_000, false),
            3_000_300_000,
        ),
        (
            999_750_000,
            1_999_500_000,
            0,
            10_000_000_000,
            applied(-250, -50, -300, -500_000, false),
            2_999_700_000,
        ),
        (
            1_000_000_000,
            2_000_000_000,
            -20_000_000,
            10_000_000_000,
            applied(0, 2000, 1000, 20_000_000, true),
            2_981_000_000,
        ),
        (
            1_000_000_000,
            2_000_000_000,
            20_000_000,
            10_000_000_000,
            applied(0, -2000, -1000, -20_000_000, true),
            3_019_000_000,
        ),
        (
            1_000_000_005,
            2_000_000_005,
            0,
            10_000_000,
            applied(0, 1, 1, 5, false),
            3_000_001_000,
        ),
        (
            999_999_995,
            1_999_999_995,
            0,
            10_000_000,
            applied(0, -1, -1, -5, false),
            2_999_999_000,
        ),
    ] {
        let mut e = estimator(DisciplineConfig {
            correction_horizon: Duration::from_nanos(horizon),
            ..Default::default()
        });
        let mut transport = normal(song);
        e.observe_clock_pair(pair(first, 1_000_000_000)).unwrap();
        assert_eq!(
            e.update(point(1, 1_000_000_000), &mut transport),
            Ok(DisciplineUpdate::Warmup { span_ns: 0 })
        );
        e.observe_clock_pair(pair(last, 2_000_000_000)).unwrap();
        assert_eq!(
            e.update(point(1, 2_000_000_000), &mut transport),
            Ok(expected)
        );
        assert_eq!(
            transport
                .position_at(Timestamp::from_nanos(2_000_000_000))
                .unwrap()
                .as_nanos(),
            song + 2_000_000_000
        );
        assert_eq!(
            transport
                .position_at(Timestamp::from_nanos(1_500_000_000))
                .unwrap()
                .as_nanos(),
            song + 1_500_000_000
        );
        assert_eq!(
            transport
                .position_at(Timestamp::from_nanos(3_000_000_000))
                .unwrap()
                .as_nanos(),
            future
        );
        assert_eq!(e.quality(), ClockMappingQuality::Unknown);
        let before = transport.clone();
        assert_eq!(
            e.update(point(1, 2_999_999_999), &mut transport),
            Ok(DisciplineUpdate::IntervalPending)
        );
        assert_eq!(transport, before);
    }
}

#[test]
fn retention_ring_decimation_and_quantized_progress_have_distinct_freshness() {
    let config = DisciplineConfig {
        capacity: 3,
        retention_interval: Duration::from_nanos(1_000_000_000),
        min_span: Duration::from_nanos(2_000_000_000),
        ..Default::default()
    };
    let mut e = estimator(config);
    assert_eq!(e.config(), config);
    assert_eq!(
        e.observe_clock_pair(pair(0, 0)),
        Ok(ObservationAdmission::Retained)
    );
    assert_eq!(
        e.observe_clock_pair(pair(400_000_000, 400_000_000)),
        Ok(ObservationAdmission::Progress)
    );
    assert_eq!(e.retained_len(), 1);
    assert_eq!(
        e.update(point(1, 400_000_000), &mut normal(0)),
        Ok(DisciplineUpdate::Warmup {
            span_ns: 400_000_000
        })
    );
    for (output, host) in [
        (1_000_000_000, 1_000_000_000),
        (2_000_000_000, 2_000_000_000),
        (3_000_300_000, 3_000_000_000),
        (4_000_600_000, 4_000_000_000),
    ] {
        assert_eq!(
            e.observe_clock_pair(pair(output, host)),
            Ok(ObservationAdmission::Retained)
        );
    }
    assert_eq!(e.retained_len(), 3);
    // Only the retained [2s, 3s, 4s] window contributes after two replacements.
    assert_eq!(
        e.update(point(1, 4_000_000_000), &mut normal(0)),
        Ok(applied(300, 60, 360, 600_000, false))
    );
    let last = e.latest_pair();
    assert_eq!(
        e.observe_clock_pair(pair(4_000_600_000, 6_000_000_000)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(e.latest_pair(), last);
    assert_eq!(e.validate_host(point(1, 6_000_000_000)), Ok(()));
    assert_eq!(
        e.validate_host(point(1, 6_000_000_001)),
        Err(EstimatorError::Stale)
    );
    assert_eq!(e.validate_host(point(1, -1)), Ok(())); // Historical queries remain permitted.

    let mut supplied = estimator(Default::default());
    let mut progress = estimator(Default::default());
    supplied.observe_clock_pair(pair(0, 100)).unwrap();
    progress.observe_progress_pair(pair(0, 100)).unwrap();
    assert_eq!(
        supplied.observe_clock_pair(pair(0, 200)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(
        progress.observe_progress_pair(pair(0, 200)),
        Ok(ObservationAdmission::Progress)
    );
    assert_eq!(supplied.latest_pair(), Some(pair(0, 100)));
    assert_eq!(progress.latest_pair(), Some(pair(0, 200)));
    assert_eq!(
        progress.observe_progress_pair(pair(0, 200)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(
        progress.observe_progress_pair(pair(1, 200)),
        Err(EstimatorError::NonIncreasing)
    );
    assert_eq!(
        progress.observe_progress_pair(pair(-1, 300)),
        Err(EstimatorError::NonIncreasing)
    );
    assert_eq!(
        supplied.validate_host(point(1, 2_000_000_200)),
        Err(EstimatorError::Stale)
    );
    assert_eq!(progress.validate_host(point(1, 2_000_000_200)), Ok(()));
    assert_eq!(
        progress.validate_host(point(1, 2_000_000_201)),
        Err(EstimatorError::Stale)
    );
    assert_eq!(progress.latest_pair(), Some(pair(0, 200)));
}

#[test]
fn rejected_pairs_and_updates_preserve_observation_and_transport_chronology() {
    let mut e = estimator(Default::default());
    let mut empty_transport = normal(0);
    assert_eq!(
        e.update(point(1, 0), &mut empty_transport),
        Err(EstimatorError::NoObservation)
    );
    assert_eq!(empty_transport, normal(0));
    e.observe_clock_pair(pair(1_000_000_000, 1_000_000_000))
        .unwrap();
    e.observe_clock_pair(pair(2_000_000_000, 2_000_000_000))
        .unwrap();
    for bad in [
        pair(1_999_999_999, 3_000_000_000),
        pair(3_000_000_000, 1_999_999_999),
        pair(3_000_000_000, 2_000_000_000),
    ] {
        assert_eq!(
            e.observe_clock_pair(bad),
            Err(EstimatorError::NonIncreasing)
        );
        assert_eq!(e.latest_pair(), Some(pair(2_000_000_000, 2_000_000_000)));
        assert_eq!(e.retained_len(), 2);
    }
    for bad in [
        ClockPair {
            source: point(3, 3_000_000_000),
            target: point(1, 3_000_000_000),
        },
        ClockPair {
            source: point(2, 3_000_000_000),
            target: point(3, 3_000_000_000),
        },
    ] {
        assert_eq!(
            e.observe_progress_pair(bad),
            Err(EstimatorError::DomainMismatch)
        );
    }
    let mut future_command = normal(0);
    future_command
        .set_rate(Timestamp::from_nanos(3_000_000_000), Rate::NORMAL)
        .unwrap();
    for (now, mut transport, error) in [
        (
            point(3, 2_000_000_000),
            normal(0),
            EstimatorError::DomainMismatch,
        ),
        (point(1, 4_000_000_001), normal(0), EstimatorError::Stale),
        (
            point(1, 2_000_000_000),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::ZERO),
            EstimatorError::NonpositiveTransport,
        ),
        (
            point(1, 2_000_000_000),
            Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::REVERSE),
            EstimatorError::NonpositiveTransport,
        ),
        (
            point(1, 2_000_000_000),
            normal(250_000_001),
            EstimatorError::PhaseErrorTooLarge,
        ),
        (
            point(1, 2_000_000_000),
            Transport::new(
                Timestamp::from_nanos(3_000_000_000),
                Timestamp::ZERO,
                Rate::NORMAL,
            ),
            EstimatorError::Transport(TransportError::BeforeOrigin),
        ),
        (
            point(1, 2_000_000_000),
            future_command,
            EstimatorError::Transport(TransportError::NonMonotonicHost),
        ),
    ] {
        let before = transport.clone();
        assert_eq!(e.update(now, &mut transport), Err(error));
        assert_eq!(transport, before);
        assert_eq!(e.latest_pair(), Some(pair(2_000_000_000, 2_000_000_000)));
        assert_eq!(e.retained_len(), 2);
    }
    let mut transport = normal(0);
    assert_eq!(
        e.update(point(1, 2_000_000_000), &mut transport),
        Ok(applied(0, 0, 0, 0, false))
    );
    let before = transport.clone();
    assert_eq!(
        e.update(point(1, 1_999_999_999), &mut transport),
        Err(EstimatorError::NonIncreasing)
    );
    assert_eq!(transport, before);
    assert_eq!(
        e.update(point(1, 3_000_000_000), &mut transport),
        Ok(applied(0, 0, 0, 0, false))
    );

    let mut drift = estimator(Default::default());
    drift
        .observe_clock_pair(pair(1_000_000_000, 1_000_000_000))
        .unwrap();
    drift
        .observe_clock_pair(pair(2_002_000_000, 2_000_000_000))
        .unwrap();
    let mut untouched = normal(0);
    assert_eq!(
        drift.update(point(1, 2_000_000_000), &mut untouched),
        Err(EstimatorError::BaseRateOutOfBounds)
    );
    assert_eq!(untouched, normal(0));
}

#[test]
fn playback_origin_signed_extremes_and_configuration_remain_checked() {
    let mut e = PresentationEstimator::new_with_playback_origin(
        Default::default(),
        point(2, -10_000_000_000),
        point(2, -8_000_000_000),
        ClockDomainId(1),
        Timestamp::from_nanos(-500_000_000),
    )
    .unwrap();
    let mut transport = Transport::new(
        Timestamp::from_nanos(-3_000_000_000),
        Timestamp::from_nanos(-500_000_000),
        Rate::NORMAL,
    );
    e.observe_clock_pair(pair(-7_000_000_000, -2_000_000_000))
        .unwrap();
    e.observe_clock_pair(pair(-6_000_000_000, -1_000_000_000))
        .unwrap();
    assert_eq!(
        e.update(point(1, -1_000_000_000), &mut transport),
        Ok(applied(0, 0, 0, 0, false))
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(-2_500_000_000))
            .unwrap(),
        Timestamp::ZERO
    );
    assert_eq!(
        transport.position_at(Timestamp::ZERO).unwrap(),
        Timestamp::from_nanos(2_500_000_000)
    );
    assert_eq!(e.latest_pair(), Some(pair(-6_000_000_000, -1_000_000_000)));
    for (song, expected) in [
        (i64::MIN, Ok(applied(0, 0, 0, 0, false))),
        (0, Err(EstimatorError::Overflow)),
    ] {
        let mut extreme = PresentationEstimator::new(
            Default::default(),
            point(2, i64::MIN),
            ClockDomainId(1),
            Timestamp::from_nanos(song),
        )
        .unwrap();
        extreme
            .observe_clock_pair(pair(i64::MIN, i64::MIN))
            .unwrap();
        extreme
            .observe_clock_pair(pair(i64::MAX, i64::MAX))
            .unwrap();
        let mut mapping = Transport::new(
            Timestamp::from_nanos(i64::MIN),
            Timestamp::from_nanos(i64::MIN),
            Rate::NORMAL,
        );
        let before = mapping.clone();
        assert_eq!(extreme.update(point(1, i64::MAX), &mut mapping), expected);
        assert_eq!(mapping.anchor(), before.anchor());
        assert_eq!(
            mapping
                .position_at(Timestamp::from_nanos(i64::MAX))
                .unwrap(),
            Timestamp::from_nanos(i64::MAX)
        );
        if expected.is_err() {
            assert_eq!(mapping, before);
        }
    }
    let default = DisciplineConfig::default();
    for config in [
        DisciplineConfig {
            capacity: 1,
            ..default
        },
        DisciplineConfig {
            capacity: 1025,
            ..default
        },
        DisciplineConfig {
            capacity: 2,
            ..default
        },
        DisciplineConfig {
            retention_interval: Duration::ZERO,
            ..default
        },
        DisciplineConfig {
            min_span: Duration::ZERO,
            ..default
        },
        DisciplineConfig {
            update_interval: Duration::ZERO,
            ..default
        },
        DisciplineConfig {
            correction_horizon: Duration::from_nanos(-1),
            ..default
        },
        DisciplineConfig {
            max_observation_age: Duration::ZERO,
            ..default
        },
        DisciplineConfig {
            max_phase_error: Duration::ZERO,
            ..default
        },
        DisciplineConfig {
            max_rate_error_ppm: 0,
            ..default
        },
        DisciplineConfig {
            max_rate_error_ppm: 1_000_000,
            ..default
        },
    ] {
        assert!(matches!(
            PresentationEstimator::new(config, point(2, 0), ClockDomainId(1), Timestamp::ZERO),
            Err(EstimatorError::InvalidConfig)
        ));
    }
    assert!(matches!(
        PresentationEstimator::new_with_playback_origin(
            default,
            point(2, 0),
            point(3, 0),
            ClockDomainId(1),
            Timestamp::ZERO
        ),
        Err(EstimatorError::DomainMismatch)
    ));
    assert!(matches!(
        PresentationEstimator::new_with_playback_origin(
            default,
            point(2, 0),
            point(2, -1),
            ClockDomainId(1),
            Timestamp::ZERO
        ),
        Err(EstimatorError::InvalidConfig)
    ));
}
