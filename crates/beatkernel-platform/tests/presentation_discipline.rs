//! Synthetic observations only; authored without native reads or test execution.
use beatkernel::{time::*, transport::*};
use beatkernel_platform::audio::{presentation::discipline::*, *};
fn host(ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(1),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn snapshot(output_ns: u64, host_ns: i64) -> AudioStreamSnapshot {
    AudioStreamSnapshot {
        telemetry_available: true,
        status: AudioStreamStatus::Running,
        counters: StreamCounters::default(),
        render: None,
        clock: Some(AudioClockSnapshot {
            position: output_ns,
            frequency: 1_000_000_000,
            qpc_100ns: host_ns.unsigned_abs() / 100,
            reading_quality: AudioClockReadingQuality::Accurate,
            host_point: Some(host(host_ns)),
            mapping_quality: ClockMappingQuality::Unknown,
        }),
    }
}
fn observer(song: i64) -> PresentationDiscipline {
    PresentationDiscipline::new(
        DisciplineConfig::default(),
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::ZERO,
        },
        ClockDomainId(1),
        Timestamp::from_nanos(song),
    )
    .unwrap()
}
fn normal(song: i64) -> Transport {
    Transport::new(Timestamp::ZERO, Timestamp::from_nanos(song), Rate::NORMAL)
}

#[test]
fn synthetic_hundred_ppm_drift_preserves_continuity_and_historical_positions() {
    let mut discipline = observer(0);
    let mut transport = normal(0);
    discipline
        .observe(snapshot(1_000_100_000, 1_000_000_000))
        .unwrap();
    assert_eq!(
        discipline
            .update(host(1_000_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Warmup { span_ns: 0 }
    );
    discipline
        .observe(snapshot(2_000_200_000, 2_000_000_000))
        .unwrap();
    let before = transport
        .position_at(host(2_000_000_000).timestamp)
        .unwrap();
    assert_eq!(
        discipline
            .update(host(2_000_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Applied {
            base_rate_ppm: 100,
            correction_ppm: 20,
            applied_rate_ppm: 120,
            phase_error_ns: 200_000,
            limited: false
        }
    );
    assert_eq!(
        transport
            .position_at(host(2_000_000_000).timestamp)
            .unwrap(),
        before
    );
    assert_eq!(
        transport
            .position_at(host(1_500_000_000).timestamp)
            .unwrap()
            .as_nanos(),
        1_500_000_000
    );
    assert_eq!(
        transport
            .position_at(host(3_000_000_000).timestamp)
            .unwrap()
            .as_nanos(),
        3_000_120_000
    );
    assert_eq!(discipline.quality(), ClockMappingQuality::Unknown);
    assert_eq!(
        discipline
            .update(host(2_500_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::IntervalPending
    );
    assert!(discipline.validate_host(host(1_500_000_000)).is_ok());
}

#[test]
fn correction_both_signs_is_limited_and_phase_converges_without_seek() {
    for initial in [-100_000_000, 100_000_000] {
        let mut discipline = observer(0);
        let mut transport = normal(initial);
        let mut first_phase = None;
        let mut last_phase = 0;
        for second in 1..=120i64 {
            discipline
                .observe(snapshot(
                    second as u64 * 1_000_000_000,
                    second * 1_000_000_000,
                ))
                .unwrap();
            let before = transport
                .position_at(host(second * 1_000_000_000).timestamp)
                .unwrap();
            let update = discipline
                .update(host(second * 1_000_000_000), &mut transport)
                .unwrap();
            assert_eq!(
                transport
                    .position_at(host(second * 1_000_000_000).timestamp)
                    .unwrap(),
                before
            );
            if let DisciplineUpdate::Applied {
                base_rate_ppm,
                correction_ppm,
                applied_rate_ppm,
                phase_error_ns,
                limited,
            } = update
            {
                assert_eq!(base_rate_ppm, 0);
                assert!(applied_rate_ppm.abs() <= 1000);
                assert_eq!(phase_error_ns.signum(), -i128::from(initial).signum());
                assert_eq!(correction_ppm.signum(), -initial.signum());
                if first_phase.is_none() {
                    first_phase = Some(phase_error_ns.abs());
                    assert!(limited);
                    assert_eq!(applied_rate_ppm, -initial.signum() * 1000);
                }
                last_phase = phase_error_ns.abs();
            }
        }
        assert!(last_phase < first_phase.unwrap() / 10);
        assert!(transport.anchors().len() > 1);
        assert_eq!(
            transport
                .position_at(host(500_000_000).timestamp)
                .unwrap()
                .as_nanos(),
            initial + 500_000_000
        );
    }
}

#[test]
fn decimation_unchanged_position_and_duplicate_do_not_refresh_progress() {
    let mut discipline = observer(0);
    let first = snapshot(1_000_000_000, 1_000_000_000);
    assert_eq!(
        discipline.observe(first).unwrap(),
        ObservationAdmission::Retained
    );
    assert_eq!(
        discipline.observe(first).unwrap(),
        ObservationAdmission::Unchanged
    );
    assert_eq!(
        discipline
            .observe(snapshot(1_010_000_000, 1_010_000_000))
            .unwrap(),
        ObservationAdmission::Progress
    );
    assert_eq!(discipline.retained_len(), 1);
    let latest = discipline.latest_pair();
    assert_eq!(
        discipline
            .observe(snapshot(1_010_000_000, 3_000_000_000))
            .unwrap(),
        ObservationAdmission::Unchanged
    );
    assert_eq!(discipline.latest_pair(), latest);
    assert_eq!(
        discipline.validate_host(host(3_010_000_001)),
        Err(DisciplineError::Stale)
    );
    assert!(discipline.validate_host(host(3_010_000_000)).is_ok());
    assert_eq!(
        discipline
            .observe(snapshot(1_100_000_000, 1_100_000_000))
            .unwrap(),
        ObservationAdmission::Retained
    );
    assert_eq!(discipline.retained_len(), 2);
    let mut transport = normal(0);
    assert_eq!(
        discipline
            .update(host(1_100_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Warmup {
            span_ns: 100_000_000
        }
    );
}

#[test]
fn admission_and_update_failures_leave_observer_and_transport_unchanged() {
    let mut discipline = observer(0);
    discipline
        .observe(snapshot(1_000_000_000, 1_000_000_000))
        .unwrap();
    discipline
        .observe(snapshot(2_000_000_000, 2_000_000_000))
        .unwrap();
    let pair = discipline.latest_pair();
    let retained = discipline.retained_len();
    for (mut bad, error) in [
        (
            snapshot(1_500_000_000, 3_000_000_000),
            DisciplineError::NonIncreasing,
        ),
        (
            snapshot(3_000_000_000, 1_900_000_000),
            DisciplineError::NonIncreasing,
        ),
        (
            snapshot(3_000_000_000, 3_000_000_000),
            DisciplineError::FrequencyChanged,
        ),
        (
            snapshot(3_000_000_000, 3_000_000_000),
            DisciplineError::DomainMismatch,
        ),
    ] {
        if error == DisciplineError::FrequencyChanged {
            bad.clock.as_mut().unwrap().frequency = 2_000_000_000;
        }
        if error == DisciplineError::DomainMismatch {
            bad.clock
                .as_mut()
                .unwrap()
                .host_point
                .as_mut()
                .unwrap()
                .domain = ClockDomainId(3);
        }
        assert_eq!(discipline.observe(bad), Err(error));
        assert_eq!(discipline.latest_pair(), pair);
        assert_eq!(discipline.retained_len(), retained);
    }
    let mut transport = normal(0);
    let original = transport.clone();
    assert_eq!(
        discipline.update(host(4_000_000_001), &mut transport),
        Err(DisciplineError::Stale)
    );
    assert_eq!(transport, original);
    transport.pause(Timestamp::ZERO).unwrap();
    let paused = transport.clone();
    assert_eq!(
        discipline.update(host(2_000_000_000), &mut transport),
        Err(DisciplineError::NonpositiveTransport)
    );
    assert_eq!(transport, paused);
    let mut transport = normal(300_000_000);
    let original = transport.clone();
    assert_eq!(
        discipline.update(host(2_000_000_000), &mut transport),
        Err(DisciplineError::PhaseErrorTooLarge)
    );
    assert_eq!(transport, original);
    let mut transport = normal(0);
    discipline
        .update(host(2_000_000_000), &mut transport)
        .unwrap();
    let original = transport.clone();
    assert_eq!(
        discipline.update(host(1_999_999_999), &mut transport),
        Err(DisciplineError::NonIncreasing)
    );
    assert_eq!(transport, original);
}

#[test]
fn drift_rejection_rounding_and_native_unit_conversion_are_explicit() {
    let mut discipline = observer(0);
    discipline
        .observe(snapshot(1_000_000_000, 1_000_000_000))
        .unwrap();
    discipline
        .observe(snapshot(2_002_000_000, 2_000_000_000))
        .unwrap();
    let mut transport = normal(0);
    let original = transport.clone();
    assert_eq!(
        discipline.update(host(2_000_000_000), &mut transport),
        Err(DisciplineError::BaseRateOutOfBounds)
    );
    assert_eq!(transport, original);
    let mut discipline = observer(0);
    let mut first = snapshot(3, 1_000_000_000);
    first.clock.as_mut().unwrap().frequency = 3;
    discipline.observe(first).unwrap();
    let mut second = snapshot(6, 2_000_000_000);
    second.clock.as_mut().unwrap().frequency = 3;
    discipline.observe(second).unwrap();
    assert_eq!(
        discipline
            .latest_pair()
            .unwrap()
            .source
            .timestamp
            .as_nanos(),
        2_000_000_000
    );
    assert!(matches!(
        discipline.update(host(2_000_000_000), &mut normal(0)),
        Ok(DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            ..
        })
    ));
    let mut discipline = observer(0);
    discipline
        .observe(snapshot(1_000_000_000, 1_000_000_000))
        .unwrap();
    discipline
        .observe(snapshot(2_000_000_500, 2_000_000_000))
        .unwrap();
    assert!(matches!(
        discipline.update(host(2_000_000_000), &mut normal(0)),
        Ok(DisciplineUpdate::Applied {
            base_rate_ppm: 1,
            ..
        })
    ));
    let mut discipline = observer(0);
    assert!(matches!(
        discipline.observe(snapshot(u64::MAX, 1)),
        Err(DisciplineError::Presentation(_))
    ));
    assert_eq!(discipline.latest_pair(), None);
}

#[test]
fn configuration_and_no_observation_failures_and_bounded_ring() {
    let default = DisciplineConfig::default();
    for config in [
        DisciplineConfig {
            capacity: 0,
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
            min_span: Duration::ZERO,
            ..default
        },
        DisciplineConfig {
            correction_horizon: Duration::from_nanos(-1),
            ..default
        },
        DisciplineConfig {
            max_rate_error_ppm: 1_000_000,
            ..default
        },
        DisciplineConfig {
            max_rate_error_ppm: 0,
            ..default
        },
    ] {
        assert!(matches!(
            PresentationDiscipline::new(config, host(0), ClockDomainId(1), Timestamp::ZERO),
            Err(DisciplineError::InvalidConfig)
        ));
    }
    let mut discipline = observer(0);
    assert_eq!(
        discipline.validate_host(host(0)),
        Err(DisciplineError::NoObservation)
    );
    assert_eq!(
        discipline.update(host(0), &mut normal(0)),
        Err(DisciplineError::NoObservation)
    );
    for second in 1..=100 {
        discipline
            .observe(snapshot(
                second * 1_000_000_000,
                second as i64 * 1_000_000_000,
            ))
            .unwrap();
    }
    assert_eq!(discipline.retained_len(), default.capacity);
    assert!(matches!(
        discipline.update(host(100_000_000_000), &mut normal(0)),
        Ok(DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            phase_error_ns: 0,
            ..
        })
    ));
}

#[test]
fn full_signed_host_span_uses_wide_arithmetic_without_inventing_overflow() {
    let mut discipline = PresentationDiscipline::new(
        DisciplineConfig::default(),
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(i64::MIN),
        },
        ClockDomainId(1),
        Timestamp::from_nanos(i64::MIN),
    )
    .unwrap();
    let mut first = snapshot(1, i64::MIN + 1);
    first.clock.as_mut().unwrap().qpc_100ns = 1;
    let mut last = snapshot(u64::MAX, i64::MAX);
    last.clock.as_mut().unwrap().qpc_100ns = 2;
    discipline.observe(first).unwrap();
    discipline.observe(last).unwrap();
    let mut transport = Transport::new(
        Timestamp::from_nanos(i64::MIN),
        Timestamp::from_nanos(i64::MIN),
        Rate::NORMAL,
    );
    assert_eq!(
        discipline.update(host(i64::MAX), &mut transport).unwrap(),
        DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            correction_ppm: 0,
            applied_rate_ppm: 0,
            phase_error_ns: 0,
            limited: false
        }
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(i64::MAX))
            .unwrap(),
        Timestamp::from_nanos(i64::MAX)
    );
}

fn supplied(output_ns: i64, host_ns: i64) -> ClockPair {
    ClockPair {
        source: ClockPoint {
            domain: ClockDomainId(2),
            timestamp: Timestamp::from_nanos(output_ns),
        },
        target: host(host_ns),
    }
}
#[test]
fn explicit_supplied_pairs_allow_zero_and_share_continuous_drift_correction() {
    let mut discipline = observer(0);
    let mut transport = normal(0);
    assert_eq!(
        discipline.observe_clock_pair(supplied(0, 0)).unwrap(),
        ObservationAdmission::Retained
    );
    assert_eq!(
        discipline.update(host(0), &mut transport).unwrap(),
        DisciplineUpdate::Warmup { span_ns: 0 }
    );
    discipline
        .observe_clock_pair(supplied(1_000_100_000, 1_000_000_000))
        .unwrap();
    let before = transport
        .position_at(Timestamp::from_nanos(1_000_000_000))
        .unwrap();
    assert_eq!(
        discipline
            .update(host(1_000_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Applied {
            base_rate_ppm: 100,
            correction_ppm: 10,
            applied_rate_ppm: 110,
            phase_error_ns: 100_000,
            limited: false
        }
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(1_000_000_000))
            .unwrap(),
        before
    );
    assert_eq!(
        transport
            .position_at(Timestamp::from_nanos(500_000_000))
            .unwrap()
            .as_nanos(),
        500_000_000
    );
    assert_eq!(discipline.quality(), ClockMappingQuality::Unknown);
}

#[test]
fn supplied_pair_domains_regressions_and_nonprogress_are_atomic() {
    let mut discipline = observer(0);
    discipline.observe_clock_pair(supplied(100, 100)).unwrap();
    let initial = discipline.latest_pair();
    assert_eq!(
        discipline.observe_clock_pair(supplied(100, 100)).unwrap(),
        ObservationAdmission::Unchanged
    );
    assert_eq!(
        discipline.observe_clock_pair(supplied(100, 200)).unwrap(),
        ObservationAdmission::Unchanged
    );
    assert_eq!(discipline.latest_pair(), initial);
    for pair in [
        supplied(99, 200),
        supplied(101, 99),
        supplied(100, 99),
        supplied(101, 100),
    ] {
        assert_eq!(
            discipline.observe_clock_pair(pair),
            Err(DisciplineError::NonIncreasing)
        );
        assert_eq!(discipline.latest_pair(), initial);
        assert_eq!(discipline.retained_len(), 1);
    }
    let mut source_domain = supplied(101, 200);
    source_domain.source.domain = ClockDomainId(3);
    let mut host_domain = supplied(101, 200);
    host_domain.target.domain = ClockDomainId(3);
    for pair in [source_domain, host_domain] {
        assert_eq!(
            discipline.observe_clock_pair(pair),
            Err(DisciplineError::DomainMismatch)
        );
        assert_eq!(discipline.latest_pair(), initial);
    }
    assert_eq!(
        discipline.validate_host(host(2_000_000_101)),
        Err(DisciplineError::Stale)
    );
    assert_eq!(
        discipline.observe_clock_pair(supplied(200, 200)).unwrap(),
        ObservationAdmission::Progress
    );
    assert_eq!(discipline.latest_pair(), Some(supplied(200, 200)));
}

#[test]
fn observations_cannot_switch_native_provenance_in_either_direction() {
    let mut supplied_observer = observer(0);
    supplied_observer
        .observe_clock_pair(supplied(1_000_000_000, 1_000_000_000))
        .unwrap();
    let original = supplied_observer.latest_pair();
    assert_eq!(
        supplied_observer.observe(snapshot(2_000_000_000, 2_000_000_000)),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(supplied_observer.latest_pair(), original);
    supplied_observer
        .observe_clock_pair(supplied(2_000_000_000, 2_000_000_000))
        .unwrap();
    let mut native_observer = observer(0);
    native_observer
        .observe(snapshot(1_000_000_000, 1_000_000_000))
        .unwrap();
    let original = native_observer.latest_pair();
    assert_eq!(
        native_observer.observe_clock_pair(supplied(2_000_000_000, 2_000_000_000)),
        Err(DisciplineError::ObservationSourceChanged)
    );
    assert_eq!(native_observer.latest_pair(), original);
    native_observer
        .observe(snapshot(2_000_000_000, 2_000_000_000))
        .unwrap();
    assert_eq!(native_observer.retained_len(), 2);
}

mod asio_observations {
    use super::*;
    use beatkernel::audio::{AudioCounters, RenderReport};
    use beatkernel_platform::audio::asio::{
        AsioPresentationError, AsioPresentationObservation, MultimediaHostInterval,
    };

    fn report(start_frame: u64) -> RenderReport {
        RenderReport {
            start_frame,
            frames: 64,
            playback_start_frame: start_frame,
            playback_frames: 64,
            paused: false,
            playback_end_physical_frame: None,
            active_voices: 0,
            pending_commands: 0,
            song_position: Timestamp::ZERO,
            producer_disconnected: false,
            counters: AudioCounters::default(),
        }
    }
    fn observation(start: u64, rate: u32, before: i64, after: i64) -> AsioPresentationObservation {
        AsioPresentationObservation::from_render(
            report(start),
            rate,
            MultimediaHostInterval {
                before: host(before),
                after: host(after),
            },
            0,
            0,
            ClockPoint {
                domain: ClockDomainId(2),
                timestamp: Timestamp::ZERO,
            },
        )
        .unwrap()
    }

    #[test]
    fn asio_midpoint_pair_retains_unknown_quality_and_continuous_normal_rate() {
        let mut discipline = observer(0);
        let mut transport = normal(0);
        let first = observation(400, 1000, 399_000_000, 401_000_000);
        assert_eq!(
            discipline.observe_asio(first),
            Ok(ObservationAdmission::Retained)
        );
        assert_eq!(
            discipline.latest_pair(),
            Some(supplied(400_000_000, 400_000_000))
        );
        assert_eq!(
            discipline.update(host(400_000_000), &mut transport),
            Ok(DisciplineUpdate::Warmup { span_ns: 0 })
        );
        let second = observation(1400, 1000, 1_399_000_000, 1_401_000_000);
        assert_eq!(
            discipline.observe_asio(second),
            Ok(ObservationAdmission::Retained)
        );
        let before = transport
            .position_at(Timestamp::from_nanos(1_400_000_000))
            .unwrap();
        assert_eq!(
            discipline.update(host(1_400_000_000), &mut transport),
            Ok(DisciplineUpdate::Applied {
                base_rate_ppm: 0,
                correction_ppm: 0,
                applied_rate_ppm: 0,
                phase_error_ns: 0,
                limited: false,
            })
        );
        assert_eq!(
            transport
                .position_at(Timestamp::from_nanos(1_400_000_000))
                .unwrap(),
            before
        );
        assert_eq!(discipline.quality(), ClockMappingQuality::Unknown);
        let mut wide = observer(0);
        wide.observe_asio(observation(0, 1000, i64::MIN, i64::MAX))
            .unwrap();
        assert_eq!(wide.latest_pair(), Some(supplied(0, -1)));
    }

    #[test]
    fn duplicate_asio_frame_never_refreshes_progress_age_even_with_later_receipt_interval() {
        let mut discipline = observer(0);
        let first = observation(400, 1000, 399_000_000, 401_000_000);
        discipline.observe_asio(first).unwrap();
        assert_eq!(
            discipline.observe_asio(first),
            Ok(ObservationAdmission::Unchanged)
        );
        let duplicate = observation(400, 1000, 3_999_000_000, 4_001_000_000);
        assert_eq!(
            discipline.observe_asio(duplicate),
            Ok(ObservationAdmission::Unchanged)
        );
        assert_eq!(
            discipline.latest_pair(),
            Some(supplied(400_000_000, 400_000_000))
        );
        assert_eq!(discipline.retained_len(), 1);
        assert_eq!(discipline.validate_host(host(2_400_000_000)), Ok(()));
        assert_eq!(
            discipline.validate_host(host(2_400_000_001)),
            Err(DisciplineError::Stale)
        );
    }

    #[test]
    fn asio_rate_change_regressed_frames_overlap_and_nonincreasing_host_are_atomic() {
        let mut discipline = observer(0);
        discipline
            .observe_asio(observation(400, 1000, 399_000_000, 401_000_000))
            .unwrap();
        let original = discipline.latest_pair();
        assert_eq!(
            discipline.observe_asio(observation(1000, 2000, 999_000_000, 1_001_000_000)),
            Err(DisciplineError::FrequencyChanged)
        );
        for invalid in [
            observation(399, 1000, 999_000_000, 1_001_000_000),
            observation(432, 1000, 999_000_000, 1_001_000_000),
            observation(1000, 1000, 398_000_000, 400_000_000),
        ] {
            assert_eq!(
                discipline.observe_asio(invalid),
                Err(DisciplineError::NonIncreasing)
            );
            assert_eq!(discipline.latest_pair(), original);
            assert_eq!(discipline.retained_len(), 1);
        }
        assert_eq!(
            discipline.observe_asio(observation(1000, 1000, 399_000_000, 401_000_000)),
            Ok(ObservationAdmission::AwaitingHostProgress)
        );
        assert_eq!(discipline.latest_pair(), original);
        assert_eq!(discipline.retained_len(), 1);
        assert_eq!(
            discipline.observe_asio(observation(1400, 1000, 1_399_000_000, 1_401_000_000)),
            Ok(ObservationAdmission::Retained)
        );
    }

    #[test]
    fn asio_origin_domains_and_forged_output_grid_fail_without_selecting_provenance() {
        let valid = observation(400, 1000, 399_000_000, 401_000_000);
        let mut origin = valid;
        origin.output_origin.timestamp = Timestamp::from_nanos(1);
        let mut host_domain = valid;
        host_domain.host.before.domain = ClockDomainId(3);
        let mut output_domain = valid;
        output_domain.output.domain = ClockDomainId(3);
        let mut output_time = valid;
        output_time.output.timestamp = Timestamp::from_nanos(400_000_001);
        let mut backwards = valid;
        backwards.host.before.timestamp = backwards
            .host
            .after
            .timestamp
            .checked_add(Duration::from_nanos(1))
            .unwrap();
        let mut no_frames = valid;
        no_frames.render.frames = 0;
        for invalid in [
            origin,
            host_domain,
            output_domain,
            output_time,
            backwards,
            no_frames,
        ] {
            let mut discipline = observer(0);
            assert!(discipline.observe_asio(invalid).is_err());
            assert_eq!(discipline.latest_pair(), None);
            assert_eq!(discipline.retained_len(), 0);
            assert_eq!(
                discipline.observe_clock_pair(supplied(0, 0)),
                Ok(ObservationAdmission::Retained)
            );
        }
        let mut discipline = observer(0);
        assert_eq!(
            discipline.observe_asio(origin),
            Err(DisciplineError::DomainMismatch)
        );
        assert_eq!(
            discipline.observe_asio(output_time),
            Err(DisciplineError::AsioPresentation(
                AsioPresentationError::Malformed
            ))
        );
        assert_eq!(
            discipline.observe_asio(valid),
            Ok(ObservationAdmission::Retained)
        );
    }

    #[test]
    fn asio_wasapi_and_supplied_pairs_cannot_mix_in_any_direction() {
        let asio = observation(1000, 1000, 999_000_000, 1_001_000_000);
        for first_native in [false, true] {
            let mut discipline = observer(0);
            if first_native {
                discipline
                    .observe(snapshot(1_000_000_000, 1_000_000_000))
                    .unwrap();
            } else {
                discipline
                    .observe_clock_pair(supplied(1_000_000_000, 1_000_000_000))
                    .unwrap();
            }
            let original = discipline.latest_pair();
            assert_eq!(
                discipline.observe_asio(asio),
                Err(DisciplineError::ObservationSourceChanged)
            );
            assert_eq!(discipline.latest_pair(), original);
        }
        for next_native in [false, true] {
            let mut discipline = observer(0);
            discipline.observe_asio(asio).unwrap();
            let original = discipline.latest_pair();
            let result = if next_native {
                discipline.observe(snapshot(2_000_000_000, 2_000_000_000))
            } else {
                discipline.observe_clock_pair(supplied(2_000_000_000, 2_000_000_000))
            };
            assert_eq!(result, Err(DisciplineError::ObservationSourceChanged));
            assert_eq!(discipline.latest_pair(), original);
            assert_eq!(discipline.retained_len(), 1);
        }
    }
}

#[test]
fn delayed_playback_origin_preserves_native_position_and_song_phase() {
    let stream = ClockPoint {
        domain: ClockDomainId(2),
        timestamp: Timestamp::ZERO,
    };
    let playback = ClockPoint {
        timestamp: Timestamp::from_nanos(5_000_000_000),
        ..stream
    };
    let mut discipline = PresentationDiscipline::new_with_playback_origin(
        DisciplineConfig::default(),
        stream,
        playback,
        ClockDomainId(1),
        Timestamp::from_nanos(-100_000_000),
    )
    .unwrap();
    let mut transport = Transport::new(
        Timestamp::from_nanos(15_000_000_000),
        Timestamp::from_nanos(-100_000_000),
        Rate::NORMAL,
    );
    discipline
        .observe(snapshot(5_100_000_000, 15_100_000_000))
        .unwrap();
    assert_eq!(
        discipline
            .latest_pair()
            .unwrap()
            .source
            .timestamp
            .as_nanos(),
        5_100_000_000
    );
    discipline
        .observe(snapshot(6_100_000_000, 16_100_000_000))
        .unwrap();
    assert_eq!(
        discipline
            .update(host(16_100_000_000), &mut transport)
            .unwrap(),
        DisciplineUpdate::Applied {
            base_rate_ppm: 0,
            correction_ppm: 0,
            applied_rate_ppm: 0,
            phase_error_ns: 0,
            limited: false
        }
    );
    assert_eq!(
        transport
            .position_at(host(16_100_000_000).timestamp)
            .unwrap()
            .as_nanos(),
        1_000_000_000
    );
    assert_eq!(
        discipline
            .latest_pair()
            .unwrap()
            .source
            .timestamp
            .as_nanos(),
        6_100_000_000
    );
    assert_eq!(discipline.quality(), ClockMappingQuality::Unknown);
    assert!(matches!(
        PresentationDiscipline::new_with_playback_origin(
            DisciplineConfig::default(),
            stream,
            ClockPoint {
                domain: ClockDomainId(3),
                ..playback
            },
            ClockDomainId(1),
            Timestamp::ZERO
        ),
        Err(DisciplineError::DomainMismatch)
    ));
    assert!(matches!(
        PresentationDiscipline::new_with_playback_origin(
            DisciplineConfig::default(),
            stream,
            ClockPoint {
                timestamp: Timestamp::from_nanos(-1),
                ..playback
            },
            ClockDomainId(1),
            Timestamp::ZERO
        ),
        Err(DisciplineError::InvalidConfig)
    ));
}
