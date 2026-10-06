//! Deferred epoch isolation and retained-ring/transport continuity fixtures.
use super::*;
use crate::transport::{Transport, Rate};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn pair(domain: u32, output: i64, host: i64) -> ClockPair {
    ClockPair {
        source: point(domain, output),
        target: point(1, host),
    }
}
fn estimator() -> PresentationEstimator {
    PresentationEstimator::new(
        DisciplineConfig::default(),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap()
}
#[test]
fn successful_epoch_rebind_clears_history_but_reuses_preallocated_ring_and_fixed_configuration() {
    let mut estimator = estimator();
    assert_eq!(estimator.epoch(), 0);
    let pointer = estimator.retained.as_ptr();
    let capacity = estimator.retained.capacity();
    for index in 0..70 {
        estimator
            .observe_clock_pair_in_epoch(0, pair(2, index * 100_000_000, index * 100_000_000))
            .unwrap();
    }
    assert_eq!(estimator.retained_len(), 64);
    assert!(estimator.next > 0);
    let mut transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
    assert!(matches!(
        estimator
            .update(point(1, 6_900_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Applied { .. }
    ));
    assert!(estimator.last_update.is_some());
    estimator
        .rebind_output(
            1,
            point(3, 10_000_000_000),
            point(3, 10_000_000_001),
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
    assert_eq!(estimator.epoch(), 1);
    assert_eq!(estimator.retained.as_ptr(), pointer);
    assert_eq!(estimator.retained.capacity(), capacity);
    assert!(estimator.latest_pair().is_none());
    assert_eq!(estimator.retained_len(), 0);
    assert_eq!(estimator.next, 0);
    assert!(estimator.last_retained.is_none());
    assert!(estimator.last_update.is_none());
    assert_eq!(estimator.config.capacity, 64);
    assert_eq!(estimator.host_domain, ClockDomainId(1));
    assert_eq!(estimator.quality(), ClockMappingQuality::Unknown);
}
#[test]
fn equal_older_domain_and_reversed_playback_rebind_refusals_are_atomic() {
    let mut estimator = estimator();
    estimator
        .rebind_output(
            5,
            point(3, 100),
            point(3, 101),
            Timestamp::from_nanos(i64::MIN),
        )
        .unwrap();
    estimator
        .observe_clock_pair_in_epoch(5, pair(3, 200, 200))
        .unwrap();
    let before = (
        estimator.epoch(),
        estimator.output_origin,
        estimator.playback_origin,
        estimator.applied_song_origin,
        estimator.latest,
        estimator.retained.clone(),
        estimator.next,
    );
    let pointer = estimator.retained.as_ptr();
    for (epoch, output, playback, expected) in [
        (5, point(4, 0), point(4, 0), EstimatorError::InvalidEpoch),
        (4, point(4, 0), point(4, 0), EstimatorError::InvalidEpoch),
        (
            6,
            point(4, 10),
            point(5, 10),
            EstimatorError::DomainMismatch,
        ),
        (6, point(4, 10), point(4, 9), EstimatorError::InvalidConfig),
    ] {
        assert_eq!(
            estimator.rebind_output(epoch, output, playback, Timestamp::ZERO),
            Err(expected)
        );
        assert_eq!(
            (
                estimator.epoch(),
                estimator.output_origin,
                estimator.playback_origin,
                estimator.applied_song_origin,
                estimator.latest,
                estimator.retained.clone(),
                estimator.next
            ),
            before
        );
        assert_eq!(estimator.retained.as_ptr(), pointer);
    }
}
#[test]
fn old_tagged_pairs_refuse_before_domain_freshness_or_progress_mutation_and_maximum_epoch_never_wraps()
 {
    let mut estimator = estimator();
    estimator
        .rebind_output(
            u64::MAX,
            point(3, i64::MIN),
            point(3, i64::MIN),
            Timestamp::from_nanos(i64::MAX),
        )
        .unwrap();
    assert_eq!(
        estimator.observe_clock_pair_in_epoch(0, pair(99, i64::MAX, i64::MAX)),
        Err(EstimatorError::EpochMismatch)
    );
    assert_eq!(
        estimator.observe_progress_pair_in_epoch(u64::MAX - 1, pair(3, 0, 0)),
        Err(EstimatorError::EpochMismatch)
    );
    assert!(estimator.latest_pair().is_none());
    assert_eq!(estimator.retained_len(), 0);
    estimator
        .observe_progress_pair_in_epoch(u64::MAX, pair(3, i64::MIN + 1, 1))
        .unwrap();
    let accepted = estimator.latest_pair();
    assert_eq!(
        estimator.rebind_output(u64::MAX, point(4, 0), point(4, 0), Timestamp::ZERO),
        Err(EstimatorError::InvalidEpoch)
    );
    assert_eq!(
        estimator.rebind_output(0, point(4, 0), point(4, 0), Timestamp::ZERO),
        Err(EstimatorError::InvalidEpoch)
    );
    assert_eq!(estimator.epoch(), u64::MAX);
    assert_eq!(estimator.latest_pair(), accepted);
    let mut cloned = estimator.clone();
    assert_eq!(cloned.epoch(), u64::MAX);
    assert_eq!(
        cloned.observe_clock_pair_in_epoch(0, pair(99, 1, 1)),
        Err(EstimatorError::EpochMismatch)
    );
    assert_eq!(cloned.latest_pair(), accepted);
}
#[test]
fn new_epoch_warms_up_again_and_applies_continuous_correction_without_rewriting_transport_history()
{
    let week = 604_800_000_000_000i64;
    let mut estimator = estimator();
    let mut transport = Transport::new(Timestamp::ZERO, Timestamp::from_nanos(week), Rate::NORMAL);
    estimator.observe_clock_pair(pair(2, 0, 0)).unwrap();
    estimator
        .observe_clock_pair(pair(2, 1_000_000_000, 1_000_000_000))
        .unwrap();
    let history = transport
        .position_at(Timestamp::from_nanos(500_000_000))
        .unwrap();
    estimator
        .rebind_output(
            1,
            point(3, 10_000_000_000),
            point(3, 10_000_000_000),
            Timestamp::from_nanos(week + 2_000_000_000),
        )
        .unwrap();
    assert_eq!(
        estimator.update(point(1, 2_000_000_000), &mut transport),
        Err(EstimatorError::NoObservation)
    );
    estimator
        .observe_clock_pair_in_epoch(1, pair(3, 10_000_000_000, 2_000_000_000))
        .unwrap();
    assert_eq!(
        estimator
            .update(point(1, 2_000_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Warmup { span_ns: 0 }
    );
    estimator
        .observe_clock_pair_in_epoch(1, pair(3, 11_000_100_000, 3_000_000_000))
        .unwrap();
    let before = transport
        .position_at(Timestamp::from_nanos(3_000_000_000))
        .unwrap();
    assert!(matches!(
        estimator
            .update(point(1, 3_000_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Applied { .. }
    ));
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(3_000_000_000))
            .unwrap(),
        before
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(500_000_000))
            .unwrap(),
        history
    );
}
