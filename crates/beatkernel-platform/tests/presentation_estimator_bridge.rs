//! Deferred memory-only adapter evidence; no drivers, clock calls or native IO.
use beatkernel::{
    audio::{AudioCounters, RenderReport},
    time::{
        presentation::PresentationEstimator, ClockDomainId, ClockMappingQuality, ClockPair,
        ClockPoint, Timestamp,
    },
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{
    asio::{AsioPresentationError, AsioPresentationObservation, MultimediaHostInterval},
    presentation::{PresentationError, discipline::*},
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
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
fn observer() -> PresentationDiscipline {
    PresentationDiscipline::new(
        Default::default(),
        point(2, 0),
        ClockDomainId(1),
        Timestamp::ZERO,
    )
    .unwrap()
}
fn normal() -> Transport {
    Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL)
}
fn snapshot(position: u64, frequency: u64, host: i64, qpc: u64) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position,
            frequency,
            qpc_100ns: qpc,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(point(1, host)),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    }
}
fn asio(frame: u64, rate: u32, before: i64, after: i64) -> AsioPresentationObservation {
    AsioPresentationObservation::from_render(
        RenderReport {
            start_frame: frame,
            frames: 16,
            playback_start_frame: frame,
            playback_frames: 16,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        },
        rate,
        MultimediaHostInterval {
            before: point(1, before),
            after: point(1, after),
        },
        0,
        0,
        point(2, 0),
    )
    .unwrap()
}

#[test]
fn native_subnanosecond_progress_is_accepted_without_turning_supplied_duplicates_into_freshness() {
    let mut native = observer();
    assert_eq!(
        native.observe(snapshot(1, 4_000_000_000, 100, 1)),
        Ok(ObservationAdmission::Retained)
    );
    assert_eq!(native.latest_pair(), Some(pair(0, 100)));
    assert_eq!(
        native.observe(snapshot(2, 4_000_000_000, 200, 2)),
        Ok(ObservationAdmission::Progress)
    );
    assert_eq!(native.latest_pair(), Some(pair(0, 200)));
    assert_eq!(native.retained_len(), 1);
    // A later receipt of the same native counter does not replace either the
    // progressing pair or its counter metadata. The next genuine step can
    // still precede that ignored receipt.
    assert_eq!(
        native.observe(snapshot(2, 4_000_000_000, 700, 3)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(native.latest_pair(), Some(pair(0, 200)));
    assert_eq!(native.validate_host(point(1, 2_000_000_200)), Ok(()));
    assert_eq!(
        native.validate_host(point(1, 2_000_000_201)),
        Err(DisciplineError::Stale)
    );
    assert_eq!(
        native.observe(snapshot(4, 4_000_000_000, 300, 3)),
        Ok(ObservationAdmission::Progress)
    );
    assert_eq!(native.latest_pair(), Some(pair(1, 300)));
    native
        .observe(snapshot(4_000_000_400, 4_000_000_000, 1_000_000_100, 4))
        .unwrap();
    assert_eq!(
        native.latest_pair(),
        Some(pair(1_000_000_100, 1_000_000_100))
    );
    assert_eq!(
        native.update(point(1, 1_000_000_100), &mut normal()),
        Ok(DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            correction_ppm: 0,
            applied_rate_ppm: 0,
            phase_error_ns: 0,
            limited: false,
        })
    );
    let mut supplied = observer();
    supplied.observe_clock_pair(pair(0, 100)).unwrap();
    assert_eq!(
        supplied.observe_clock_pair(pair(0, 200)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(supplied.latest_pair(), Some(pair(0, 100)));
    assert_eq!(
        supplied.validate_host(point(1, 2_000_000_200)),
        Err(DisciplineError::Stale)
    );

    // Native forwarding and the core share these types, but the expected
    // values come from +/-250us per second and +/-500us over a 10s horizon.
    for (first, last, base, correction, phase, future) in [
        (
            1_000_250_000,
            2_000_500_000,
            250,
            50,
            500_000,
            3_000_300_000,
        ),
        (
            999_750_000,
            1_999_500_000,
            -250,
            -50,
            -500_000,
            2_999_700_000,
        ),
    ] {
        let mut native = observer();
        let mut core = PresentationEstimator::new(
            DisciplineConfig::default(),
            point(2, 0),
            ClockDomainId(1),
            Timestamp::ZERO,
        )
        .unwrap();
        let mut a = normal();
        let mut b = normal();
        native
            .observe(snapshot(first as u64, 1_000_000_000, 1_000_000_000, 10))
            .unwrap();
        native
            .observe(snapshot(last as u64, 1_000_000_000, 2_000_000_000, 20))
            .unwrap();
        core.observe_clock_pair(pair(first, 1_000_000_000)).unwrap();
        core.observe_clock_pair(pair(last, 2_000_000_000)).unwrap();
        let expected = DisciplineUpdate::Applied {
            base_rate_ppm: base,
            correction_ppm: correction,
            applied_rate_ppm: base + correction,
            phase_error_ns: phase,
            limited: false,
        };
        assert_eq!(native.update(point(1, 2_000_000_000), &mut a), Ok(expected));
        assert_eq!(core.update(point(1, 2_000_000_000), &mut b), Ok(expected));
        assert_eq!(a, b);
        assert_eq!(
            a.position_at(Timestamp::from_nanos(3_000_000_000))
                .unwrap()
                .as_nanos(),
            future
        );
        assert_eq!(native.quality(), ClockMappingQuality::Unknown);
    }
}

#[test]
fn malformed_native_snapshots_and_failed_admissions_do_not_select_or_advance_source_identity() {
    let valid = snapshot(1_000_000_000, 1_000_000_000, 1_000_000_000, 10);
    for case in 0..8 {
        let mut bad = valid;
        let expected = match case {
            0 => {
                bad.telemetry_available = false;
                DisciplineError::Presentation(PresentationError::Unavailable)
            }
            1 => {
                bad.clock = None;
                DisciplineError::Presentation(PresentationError::Unavailable)
            }
            2 => {
                bad.clock.as_mut().unwrap().host_point = None;
                DisciplineError::Presentation(PresentationError::Unavailable)
            }
            3 => {
                bad.clock.as_mut().unwrap().position = 0;
                DisciplineError::Presentation(PresentationError::BeforePresentation)
            }
            4 => {
                bad.clock.as_mut().unwrap().frequency = 0;
                DisciplineError::Presentation(PresentationError::FrequencyChanged)
            }
            5 => {
                bad.clock.as_mut().unwrap().reading_quality = AudioClockReadingQuality::Degraded;
                DisciplineError::Presentation(PresentationError::Inaccurate)
            }
            6 => {
                bad.clock.as_mut().unwrap().host_point = Some(point(9, 1_000_000_000));
                DisciplineError::DomainMismatch
            }
            _ => {
                let clock = bad.clock.as_mut().unwrap();
                clock.position = u64::MAX;
                clock.frequency = 1;
                DisciplineError::Presentation(PresentationError::Overflow)
            }
        };
        let mut native = observer();
        assert_eq!(native.observe(bad), Err(expected));
        assert_eq!(native.latest_pair(), None);
        assert_eq!(native.retained_len(), 0);
        assert_eq!(
            native.observe_clock_pair(pair(0, 0)),
            Ok(ObservationAdmission::Retained)
        );
    }
    let mut native = observer();
    native.observe(valid).unwrap();
    for (bad, expected) in [
        (
            snapshot(3_000_000_000, 2_000_000_000, 3_000_000_000, 30),
            DisciplineError::FrequencyChanged,
        ),
        (
            snapshot(999_999_999, 1_000_000_000, 2_000_000_000, 20),
            DisciplineError::NonIncreasing,
        ),
        (
            snapshot(2_000_000_000, 1_000_000_000, 2_000_000_000, 10),
            DisciplineError::NonIncreasing,
        ),
        (
            snapshot(2_000_000_000, 1_000_000_000, 1_000_000_000, 20),
            DisciplineError::NonIncreasing,
        ),
    ] {
        assert_eq!(native.observe(bad), Err(expected));
        assert_eq!(
            native.latest_pair(),
            Some(pair(1_000_000_000, 1_000_000_000))
        );
        assert_eq!(native.retained_len(), 1);
    }
    assert_eq!(
        native.observe(snapshot(2_000_000_000, 1_000_000_000, 2_000_000_000, 11)),
        Ok(ObservationAdmission::Retained)
    );
    let mut transport = normal();
    let before = transport.clone();
    assert_eq!(
        native.update(point(1, 4_000_000_001), &mut transport),
        Err(DisciplineError::Stale)
    );
    assert_eq!(transport, before);
    assert_eq!(
        native.update(point(1, 2_000_000_000), &mut transport),
        Ok(DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            correction_ppm: 0,
            applied_rate_ppm: 0,
            phase_error_ns: 0,
            limited: false,
        })
    );
    let before = transport.clone();
    assert_eq!(
        native.update(point(1, 1_999_999_999), &mut transport),
        Err(DisciplineError::NonIncreasing)
    );
    assert_eq!(transport, before);
}

#[test]
fn asio_coarse_midpoint_waits_without_committing_frames_and_all_native_sources_remain_exclusive() {
    let first = asio(1000, 1000, 999_999_998, 1_000_000_002);
    let mut native = observer();
    native.observe_asio(first).unwrap();
    assert_eq!(
        native.latest_pair(),
        Some(pair(1_000_000_000, 1_000_000_000))
    );
    let awaiting = asio(2000, 1000, 999_999_999, 1_000_000_001);
    assert_eq!(
        native.observe_asio(awaiting),
        Ok(ObservationAdmission::AwaitingHostProgress)
    );
    assert_eq!(
        native.latest_pair(),
        Some(pair(1_000_000_000, 1_000_000_000))
    );
    assert_eq!(native.retained_len(), 1);
    assert_eq!(
        native.validate_host(point(1, 3_000_000_001)),
        Err(DisciplineError::Stale)
    );
    // This is after the accepted 1000..1016 block but before the unadmitted
    // 2000..2016 block, proving the latter did not move retained frame state.
    assert_eq!(
        native.observe_asio(asio(1016, 1000, 1_015_999_998, 1_016_000_002)),
        Ok(ObservationAdmission::Progress)
    );
    assert_eq!(
        native.observe_asio(asio(2000, 1000, 1_999_999_998, 2_000_000_002)),
        Ok(ObservationAdmission::Retained)
    );
    let accepted = native.latest_pair();
    assert_eq!(
        native.observe_asio(asio(2000, 1000, 4_999_999_998, 5_000_000_002)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(native.latest_pair(), accepted);
    assert_eq!(
        native.observe_asio(asio(3000, 2000, 2_999_999_998, 3_000_000_002)),
        Err(DisciplineError::FrequencyChanged)
    );
    assert_eq!(
        native.observe_asio(asio(2008, 1000, 2_999_999_998, 3_000_000_002)),
        Err(DisciplineError::NonIncreasing)
    );
    let mut malformed = asio(3000, 1000, 2_999_999_998, 3_000_000_002);
    malformed.output.timestamp = Timestamp::from_nanos(3_000_000_001);
    assert_eq!(
        native.observe_asio(malformed),
        Err(DisciplineError::AsioPresentation(
            AsioPresentationError::Malformed
        ))
    );
    assert_eq!(native.latest_pair(), accepted);
    assert_eq!(native.retained_len(), 2);
    assert_eq!(
        native.update(point(1, 2_000_000_000), &mut normal()),
        Ok(DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            correction_ppm: 0,
            applied_rate_ppm: 0,
            phase_error_ns: 0,
            limited: false,
        })
    );
    for first_source in 0..3 {
        let mut adapter = observer();
        match first_source {
            0 => {
                adapter
                    .observe(snapshot(1_000_000_000, 1_000_000_000, 1_000_000_000, 10))
                    .unwrap();
            }
            1 => {
                adapter
                    .observe_clock_pair(pair(1_000_000_000, 1_000_000_000))
                    .unwrap();
            }
            _ => {
                adapter.observe_asio(first).unwrap();
            }
        }
        for next_source in 0..3 {
            if next_source == first_source {
                continue;
            }
            let result = match next_source {
                0 => adapter.observe(snapshot(2_000_000_000, 1_000_000_000, 2_000_000_000, 20)),
                1 => adapter.observe_clock_pair(pair(2_000_000_000, 2_000_000_000)),
                _ => adapter.observe_asio(asio(2000, 1000, 1_999_999_998, 2_000_000_002)),
            };
            assert_eq!(result, Err(DisciplineError::ObservationSourceChanged));
            assert_eq!(
                adapter.latest_pair(),
                Some(pair(1_000_000_000, 1_000_000_000))
            );
            assert_eq!(adapter.retained_len(), 1);
        }
    }
    let mut wide = observer();
    wide.observe_asio(asio(0, 1000, i64::MIN, i64::MAX))
        .unwrap();
    assert_eq!(wide.latest_pair(), Some(pair(0, -1)));
    assert_eq!(wide.quality(), ClockMappingQuality::Unknown);
}
