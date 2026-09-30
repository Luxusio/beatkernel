use beatkernel::{time::*, transport::Rate};
use beatkernel_platform::audio::{presentation::*, *};
fn snapshot(position: u64, host_ns: i64) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency: 1000,
            qpc_100ns: host_ns as u64 / 100,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(ClockPoint {
                domain: ClockDomainId(1),
                timestamp: Timestamp::from_nanos(host_ns),
            }),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    }
}
fn build(
    first: AudioStreamSnapshot,
    second: AudioStreamSnapshot,
) -> Result<WasapiPresentationClock, PresentationError> {
    WasapiPresentationClock::from_snapshots(
        first,
        second,
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::ZERO,
        },
        ClockInterval {
            start: Timestamp::ZERO,
            end: Timestamp::from_nanos(2_000_000_000),
        },
        ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(100_000_000),
            after: Duration::from_nanos(2_000_000_000),
        },
        CalibrationUncertainty {
            observation_error: Duration::from_nanos(100),
            residual_drift_error: None,
        },
    )
}
#[test]
fn native_position_frequency_drives_origin_and_inverse_transport_slope() {
    let relation = build(snapshot(100, 1_000_000_000), snapshot(200, 1_110_000_000)).unwrap();
    assert_eq!(relation.quality(), ClockMappingQuality::Unknown);
    let transport = relation
        .transport(Timestamp::from_nanos(20_000_000_000))
        .unwrap();
    assert_eq!(
        transport.anchor().host_time,
        Timestamp::from_nanos(890_000_000)
    );
    assert_eq!(transport.anchor().rate, Rate::new(10, 11).unwrap());
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(1_110_000_000))
            .unwrap(),
        Timestamp::from_nanos(20_200_000_000)
    );
    assert!(relation
        .mapper()
        .map(
            ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::from_nanos(3_000_000_000)
            },
            ClockDomainId(1)
        )
        .is_none());
}
#[test]
fn prestart_degraded_stale_reset_and_frequency_change_are_not_promoted() {
    assert_eq!(
        build(snapshot(0, 1_000_000_000), snapshot(200, 1_100_000_000)).unwrap_err(),
        PresentationError::BeforePresentation
    );
    let mut bad = snapshot(100, 1_000_000_000);
    bad.clock.as_mut().unwrap().reading_quality = AudioClockReadingQuality::Degraded;
    assert_eq!(
        build(bad, snapshot(200, 1_100_000_000)).unwrap_err(),
        PresentationError::Inaccurate
    );
    assert_eq!(
        build(snapshot(100, 1_000_000_000), snapshot(100, 1_100_000_000)).unwrap_err(),
        PresentationError::NonIncreasing
    );
    let mut changed = snapshot(200, 1_100_000_000);
    changed.clock.as_mut().unwrap().frequency = 48000;
    assert_eq!(
        build(snapshot(100, 1_000_000_000), changed).unwrap_err(),
        PresentationError::FrequencyChanged
    );
}
