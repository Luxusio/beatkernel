//! Deferred memory observations; no native device acquisition.
use beatkernel::{audio::OutputFrameBasis, time::*};
use beatkernel_platform::audio::{
    *,
    presentation::{*, discipline::*},
};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn snapshot(position: u64, frequency: u64, host: i64) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency,
            qpc_100ns: host as u64 / 100,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(point(1, host)),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    }
}
#[test]
fn wasapi_arbitrary_counter_frequency_preserves_fractional_frame_carry_and_calibrated_zero() {
    let basis = OutputFrameBasis::new(point(2, 0), 3, 2).unwrap();
    let first = snapshot(1, 3, 1_000_000_000);
    let second = snapshot(4, 3, 2_000_000_000);
    let (pair, frequency, position, _) = observation_with_basis(first, basis).unwrap();
    assert_eq!(pair.source, point(2, 1_000_000_000));
    assert_eq!((frequency, position), (3, 1));
    let clock = WasapiPresentationClock::from_snapshots_with_basis(
        first,
        second,
        basis,
        ClockInterval {
            start: Timestamp::ZERO,
            end: Timestamp::from_nanos(3_000_000_000),
        },
        ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(1_000_000_000),
            after: Duration::from_nanos(1_000_000_000),
        },
        CalibrationUncertainty {
            observation_error: Duration::ZERO,
            residual_drift_error: None,
        },
    )
    .unwrap();
    assert_eq!(clock.output_origin(), point(2, 666_666_666));
    assert_eq!(
        clock
            .mapper()
            .map_checked(point(2, 1_000_000_000), ClockDomainId(1))
            .unwrap(),
        Timestamp::from_nanos(1_000_000_000)
    );
    let zero = OutputFrameBasis::new(point(2, -123), 44_100, 0).unwrap();
    let observed = observation_with_basis(snapshot(100, 1000, 1_000_000_000), zero).unwrap();
    assert_eq!(observed.0.source, point(2, 99_999_877));
}
#[test]
fn malformed_native_observation_is_not_repaired_by_a_valid_basis() {
    let basis = OutputFrameBasis::new(point(2, 0), 3, 2).unwrap();
    assert_eq!(
        observation_with_basis(snapshot(0, 3, 100), basis),
        Err(PresentationError::BeforePresentation)
    );
    assert_eq!(
        observation_with_basis(snapshot(1, 0, 100), basis),
        Err(PresentationError::FrequencyChanged)
    );
    let mut bad = snapshot(1, 3, 100);
    bad.telemetry_available = false;
    assert_eq!(
        observation_with_basis(bad, basis),
        Err(PresentationError::Unavailable)
    );
    bad = snapshot(1, 3, 100);
    bad.clock.as_mut().unwrap().reading_quality = AudioClockReadingQuality::Unknown;
    assert_eq!(
        observation_with_basis(bad, basis),
        Err(PresentationError::Inaccurate)
    );
}
#[test]
fn basis_identity_and_frequency_cannot_change_inside_epoch_and_stale_tag_cannot_claim_new_identity()
{
    let mut observer = PresentationDiscipline::new(
        DisciplineConfig::default(),
        point(2, 666_666_666),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap();
    let basis = OutputFrameBasis::new(point(2, 0), 3, 2).unwrap();
    observer
        .observe_with_basis_in_epoch(0, snapshot(1, 3, 1_000_000_000), basis)
        .unwrap();
    let pair = observer.latest_pair();
    let retained = observer.retained_len();
    let same_zero_different_grid = OutputFrameBasis::new(point(2, 0), 6, 4).unwrap();
    assert_eq!(
        observer.observe_with_basis_in_epoch(
            0,
            snapshot(2, 3, 2_000_000_000),
            same_zero_different_grid
        ),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(observer.latest_pair(), pair);
    assert_eq!(observer.retained_len(), retained);
    for changed in [
        OutputFrameBasis::new(point(2, 0), 3, 3).unwrap(),
        OutputFrameBasis::new(point(2, 0), 4, 2).unwrap(),
    ] {
        assert!(
            observer
                .observe_with_basis_in_epoch(0, snapshot(2, 3, 2_000_000_000), changed)
                .is_err()
        );
        assert_eq!(observer.latest_pair(), pair);
        assert_eq!(observer.retained_len(), retained);
    }
    assert_eq!(
        observer.observe_with_basis_in_epoch(0, snapshot(2, 4, 2_000_000_000), basis),
        Err(DisciplineError::FrequencyChanged)
    );
    assert!(observer.observe(snapshot(2, 3, 2_000_000_000)).is_err());
    assert_eq!(observer.latest_pair(), pair);
    observer
        .rebind_output(
            1,
            point(2, 1_750_000_000),
            point(2, 1_750_000_000),
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
    assert_eq!(
        observer.observe_with_basis_in_epoch(0, snapshot(0, 0, 0), basis),
        Err(DisciplineError::EpochMismatch)
    );
    assert!(observer.latest_pair().is_none());
    let next = OutputFrameBasis::new(point(2, 0), 4, 7).unwrap();
    observer
        .observe_with_basis_in_epoch(1, snapshot(1, 4, 3_000_000_000), next)
        .unwrap();
    assert_eq!(
        observer.latest_pair().unwrap().source,
        point(2, 2_000_000_000)
    );
}
#[cfg(target_os = "linux")]
#[test]
fn alsa_verified_played_frames_add_offset_before_floor_and_keep_native_validation() {
    use beatkernel_platform::linux::*;
    let mut evidence = AlsaTimingSnapshot {
        native_state: 3,
        submitted_frames: 8,
        delay_frames: 7,
        available_frames: 99,
        native_htstamp: AlsaNativeTimestamp {
            seconds: 1,
            nanoseconds: 123,
        },
        native_timestamp: Some(point(1, 1_000_000_123)),
        query_started: point(1, 2_000_000_000),
        query_finished: point(1, 2_000_000_100),
        estimated_played_frames: Some(1),
        quality: ClockMappingQuality::Unknown,
        timestamp_mode: 1,
        timestamp_type: 1,
    };
    let basis = OutputFrameBasis::new(point(2, 0), 3, 2).unwrap();
    assert_eq!(
        alsa_presentation_pair_with_basis(evidence, basis)
            .unwrap()
            .unwrap(),
        ClockPair {
            source: point(2, 1_000_000_000),
            target: point(1, 1_000_000_123)
        }
    );
    let zero = OutputFrameBasis::new(point(2, -123), 3, 0).unwrap();
    assert_eq!(
        alsa_presentation_pair_with_basis(evidence, zero).unwrap(),
        alsa_presentation_pair(evidence, point(2, -123), 3).unwrap()
    );
    evidence.delay_frames = -1;
    assert!(alsa_presentation_pair_with_basis(evidence, basis).is_err());
    evidence.delay_frames = 7;
    evidence.query_finished = point(3, 2_000_000_100);
    assert!(alsa_presentation_pair_with_basis(evidence, basis).is_err());
    evidence.query_finished = point(1, 1);
    assert!(alsa_presentation_pair_with_basis(evidence, basis).is_err());
    evidence.estimated_played_frames = None;
    assert_eq!(
        alsa_presentation_pair_with_basis(evidence, basis).unwrap(),
        None
    );
}
