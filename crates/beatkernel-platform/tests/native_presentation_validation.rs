//! Original native metadata transactions, authored independently of estimator extraction.
use beatkernel::{
    audio::{AudioCounters, OutputFrameBasis, RenderReport},
    time::{
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Duration, Timestamp,
        presentation::PresentationEstimator,
    },
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{
    AudioClockReadingQuality, AudioClockSnapshot, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
    asio::{AsioPresentationError, AsioPresentationObservation, MultimediaHostInterval},
    presentation::{
        PresentationError,
        discipline::{DisciplineConfig, DisciplineError, DisciplineUpdate, PresentationDiscipline},
        validation::*,
    },
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
fn validator() -> NativePresentationValidator {
    NativePresentationValidator::new(0, point(2, 0), ClockDomainId(1))
}
fn state(v: &NativePresentationValidator) -> String {
    format!(
        "{:?}",
        (
            v.epoch(),
            v.output_origin(),
            v.host_domain(),
            v.latest_record(),
            v.latest_pair()
        )
    )
}
fn commit_native(
    v: &mut NativePresentationValidator,
    s: AudioStreamSnapshot,
) -> NativeObservationAdmission {
    let prepared = v.prepare_wasapi(v.epoch(), s, None).unwrap();
    v.commit(prepared).unwrap()
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
fn wasapi_preparation_retains_original_record_and_native_subnanosecond_progress() {
    let mut v = validator();
    let first = snapshot(1, 4_000_000_000, 100, 1);
    let before = state(&v);
    let prepared = v.prepare_wasapi(0, first, None).unwrap();
    assert_eq!(state(&v), before);
    assert_eq!(prepared.admission(), NativeObservationAdmission::Progress);
    assert_eq!(prepared.correlation_pair(), Some(pair(0, 100)));
    assert_eq!(
        prepared.evidence(),
        &OriginalNativePresentationEvidence::Wasapi {
            snapshot: first,
            basis: None
        }
    );
    assert_eq!(v.commit(prepared), Ok(NativeObservationAdmission::Progress));
    assert_eq!(v.latest_pair(), Some(pair(0, 100)));
    assert_eq!(
        commit_native(&mut v, snapshot(2, 4_000_000_000, 200, 2)),
        NativeObservationAdmission::Progress
    );
    assert_eq!(v.latest_pair(), Some(pair(0, 200)));
    let before = state(&v);
    let duplicate = v
        .prepare_wasapi(0, snapshot(2, 4_000_000_000, 700, 3), None)
        .unwrap();
    assert_eq!(duplicate.admission(), NativeObservationAdmission::Unchanged);
    assert_eq!(duplicate.correlation_pair(), None);
    assert_eq!(
        v.commit(duplicate),
        Ok(NativeObservationAdmission::Unchanged)
    );
    assert_eq!(state(&v), before);
    assert_eq!(
        commit_native(&mut v, snapshot(4, 4_000_000_000, 300, 3)),
        NativeObservationAdmission::Progress
    );
    assert_eq!(v.latest_pair(), Some(pair(1, 300)));
    let record = v.latest_record().unwrap();
    assert_eq!(record.pair(), pair(1, 300));
    assert_eq!(
        record.evidence(),
        &OriginalNativePresentationEvidence::Wasapi {
            snapshot: snapshot(4, 4_000_000_000, 300, 3),
            basis: None
        }
    );
}

#[test]
fn wasapi_frequency_qpc_position_domain_and_source_errors_preserve_state_and_precedence() {
    let mut v = validator();
    commit_native(&mut v, snapshot(100, 1_000_000_000, 100, 1));
    let cases = [
        (
            snapshot(90, 2_000_000_000, 90, 0),
            DisciplineError::FrequencyChanged,
        ),
        (
            snapshot(99, 1_000_000_000, 200, 2),
            DisciplineError::NonIncreasing,
        ),
        (
            snapshot(101, 1_000_000_000, 200, 1),
            DisciplineError::NonIncreasing,
        ),
        (
            snapshot(101, 1_000_000_000, 100, 2),
            DisciplineError::NonIncreasing,
        ),
    ];
    for (received, expected) in cases {
        let before = state(&v);
        assert_eq!(v.prepare_wasapi(0, received, None).unwrap_err(), expected);
        assert_eq!(state(&v), before);
    }
    let mut bad = snapshot(101, 1_000_000_000, 200, 2);
    bad.clock.as_mut().unwrap().host_point = Some(point(3, 200));
    let before = state(&v);
    assert_eq!(
        v.prepare_wasapi(0, bad, None).unwrap_err(),
        DisciplineError::DomainMismatch
    );
    assert_eq!(state(&v), before);
    assert_eq!(
        v.prepare_pair(0, pair(200, 200)).unwrap_err(),
        DisciplineError::ObservationSourceChanged
    );
    assert_eq!(
        v.prepare_pair(
            0,
            ClockPair {
                source: point(3, 200),
                target: point(1, 200)
            }
        )
        .unwrap_err(),
        DisciplineError::DomainMismatch
    );
    let mut unavailable = snapshot(0, 0, 0, 0);
    unavailable.telemetry_available = false;
    assert_eq!(
        v.prepare_wasapi(1, unavailable, None).unwrap_err(),
        DisciplineError::EpochMismatch
    );
    assert_eq!(
        v.prepare_wasapi(0, unavailable, None).unwrap_err(),
        DisciplineError::Presentation(PresentationError::Unavailable)
    );
    let wrong_basis = OutputFrameBasis::new(point(3, 0), 1000, 0).unwrap();
    assert_eq!(
        v.prepare_wasapi(0, unavailable, Some(wrong_basis))
            .unwrap_err(),
        DisciplineError::DomainMismatch
    );
    let mut supplied = validator();
    let prepared = supplied.prepare_pair(0, pair(100, 100)).unwrap();
    supplied.commit(prepared).unwrap();
    assert_eq!(
        supplied.prepare_wasapi(0, unavailable, None).unwrap_err(),
        DisciplineError::ObservationSourceChanged
    );
    assert_eq!(state(&v), before);
}

#[test]
fn standalone_snapshot_errors_and_native_conversion_overflow_are_explicit() {
    let v = validator();
    let initial = state(&v);
    let mut unavailable = snapshot(1, 1, 100, 1);
    unavailable.clock = None;
    assert_eq!(
        v.prepare_wasapi(0, unavailable, None).unwrap_err(),
        DisciplineError::Presentation(PresentationError::Unavailable)
    );
    let mut inaccurate = snapshot(0, 0, 100, 1);
    inaccurate.clock.as_mut().unwrap().reading_quality = AudioClockReadingQuality::Degraded;
    assert_eq!(
        v.prepare_wasapi(0, inaccurate, None).unwrap_err(),
        DisciplineError::Presentation(PresentationError::Inaccurate)
    );
    assert_eq!(
        v.prepare_wasapi(0, snapshot(0, 0, 100, 1), None)
            .unwrap_err(),
        DisciplineError::Presentation(PresentationError::BeforePresentation)
    );
    assert_eq!(
        v.prepare_wasapi(0, snapshot(1, 0, 100, 1), None)
            .unwrap_err(),
        DisciplineError::Presentation(PresentationError::FrequencyChanged)
    );
    assert_eq!(
        v.prepare_wasapi(0, snapshot(u64::MAX, 1, 100, 1), None)
            .unwrap_err(),
        DisciplineError::Presentation(PresentationError::Overflow)
    );
    assert_eq!(state(&v), initial);
}

#[test]
fn basis_identity_is_original_grid_identity_even_when_stream_zero_is_equal() {
    let mut v = NativePresentationValidator::new(0, point(2, 666_666_666), ClockDomainId(1));
    let basis = OutputFrameBasis::new(point(2, 0), 3, 2).unwrap();
    let prepared = v
        .prepare_wasapi(0, snapshot(1, 3, 1_000_000_000, 1), Some(basis))
        .unwrap();
    v.commit(prepared).unwrap();
    let before = state(&v);
    let other = OutputFrameBasis::new(point(2, 0), 6, 4).unwrap();
    assert_eq!(
        v.prepare_wasapi(0, snapshot(2, 3, 2_000_000_000, 2), Some(other))
            .unwrap_err(),
        DisciplineError::ObservationSourceChanged
    );
    assert_eq!(state(&v), before);
    assert_eq!(
        v.prepare_wasapi(1, snapshot(0, 0, 0, 0), Some(other))
            .unwrap_err(),
        DisciplineError::EpochMismatch
    );
}

#[test]
fn asio_keeps_full_bracket_and_render_while_equal_midpoint_defers_without_mutation() {
    let mut v = validator();
    let first = asio(0, 1000, 90, 110);
    let prepared = v.prepare_asio(0, first, None).unwrap();
    assert_eq!(prepared.correlation_pair(), Some(pair(0, 100)));
    assert_eq!(
        prepared.evidence(),
        &OriginalNativePresentationEvidence::Asio {
            observation: first,
            basis: None
        }
    );
    v.commit(prepared).unwrap();
    let before = state(&v);
    let duplicate = v.prepare_asio(0, first, None).unwrap();
    assert_eq!(duplicate.admission(), NativeObservationAdmission::Unchanged);
    v.commit(duplicate).unwrap();
    assert_eq!(state(&v), before);
    let coarse = asio(16, 1000, 95, 105);
    let deferred = v.prepare_asio(0, coarse, None).unwrap();
    assert_eq!(
        deferred.admission(),
        NativeObservationAdmission::AwaitingHostProgress
    );
    assert_eq!(deferred.correlation_pair(), None);
    assert_eq!(
        deferred.evidence(),
        &OriginalNativePresentationEvidence::Asio {
            observation: coarse,
            basis: None
        }
    );
    assert_eq!(
        v.commit(deferred),
        Ok(NativeObservationAdmission::AwaitingHostProgress)
    );
    assert_eq!(state(&v), before);
    let next = asio(16, 1000, 190, 230);
    let prepared = v.prepare_asio(0, next, None).unwrap();
    assert_eq!(prepared.correlation_pair(), Some(pair(16_000_000, 210)));
    v.commit(prepared).unwrap();
    let record = v.latest_record().unwrap();
    assert_eq!(record.pair(), pair(16_000_000, 210));
    assert_eq!(
        record.evidence(),
        &OriginalNativePresentationEvidence::Asio {
            observation: next,
            basis: None
        }
    );
    assert_eq!(next.host.before, point(1, 190));
    assert_eq!(next.host.after, point(1, 230));
    assert_eq!(next.render.frames, 16);
}

#[test]
fn asio_rate_overlap_domain_output_extent_and_source_errors_are_atomic() {
    let mut v = validator();
    let first = v.prepare_asio(0, asio(0, 1000, 90, 110), None).unwrap();
    v.commit(first).unwrap();
    let before = state(&v);
    for (received, expected) in [
        (asio(16, 2000, 190, 210), DisciplineError::FrequencyChanged),
        (asio(8, 1000, 190, 210), DisciplineError::NonIncreasing),
        (asio(16, 1000, 80, 100), DisciplineError::NonIncreasing),
    ] {
        assert_eq!(v.prepare_asio(0, received, None).unwrap_err(), expected);
        assert_eq!(state(&v), before);
    }
    let mut bad = asio(16, 1000, 190, 210);
    bad.output.timestamp = Timestamp::from_nanos(16_000_001);
    assert_eq!(
        v.prepare_asio(0, bad, None).unwrap_err(),
        DisciplineError::AsioPresentation(AsioPresentationError::Malformed)
    );
    let mut bad = asio(16, 1000, 190, 210);
    bad.host.after = point(3, 210);
    assert_eq!(
        v.prepare_asio(0, bad, None).unwrap_err(),
        DisciplineError::DomainMismatch
    );
    let mut bad = asio(16, 1000, 190, 210);
    bad.render.frames = 0;
    assert_eq!(
        v.prepare_asio(0, bad, None).unwrap_err(),
        DisciplineError::AsioPresentation(AsioPresentationError::Malformed)
    );
    let mut bad = asio(16, 1000, 190, 210);
    bad.sample_rate = 0;
    assert_eq!(
        v.prepare_asio(0, bad, None).unwrap_err(),
        DisciplineError::AsioPresentation(AsioPresentationError::Malformed)
    );
    let mut bad = asio(16, 1000, 190, 210);
    bad.host.before = point(1, 211);
    assert_eq!(
        v.prepare_asio(0, bad, None).unwrap_err(),
        DisciplineError::AsioPresentation(AsioPresentationError::Malformed)
    );
    let mut bad = asio(16, 1000, 190, 210);
    bad.render.start_frame = u64::MAX;
    assert_eq!(
        v.prepare_asio(0, bad, None).unwrap_err(),
        DisciplineError::AsioPresentation(AsioPresentationError::Overflow)
    );
    assert_eq!(
        v.prepare_pair(0, pair(16_000_000, 200)).unwrap_err(),
        DisciplineError::ObservationSourceChanged
    );
    assert_eq!(state(&v), before);
}

#[test]
fn prepared_tokens_bind_original_prior_record_and_reject_stale_or_cross_owner_state() {
    let mut a = validator();
    let first = snapshot(100, 1_000_000_000, 100, 1);
    commit_native(&mut a, first);
    let next = a
        .prepare_wasapi(0, snapshot(200, 1_000_000_000, 200, 2), None)
        .unwrap();
    let mut different = validator();
    let mut changed = first;
    changed.clock.as_mut().unwrap().mapping_quality = ClockMappingQuality::Estimated {
        max_error: Duration::from_nanos(1),
    };
    commit_native(&mut different, changed);
    assert_eq!(different.latest_pair(), a.latest_pair());
    let before = state(&different);
    assert_eq!(
        different.commit(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(state(&different), before);
    let mut identical = validator();
    commit_native(&mut identical, first);
    assert_eq!(
        identical.commit(next),
        Ok(NativeObservationAdmission::Progress)
    );
    commit_native(&mut a, snapshot(300, 1_000_000_000, 300, 3));
    let before = state(&a);
    assert_eq!(
        a.commit(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(state(&a), before);
}

#[test]
fn rebind_is_staged_preserves_host_and_rejects_retired_or_stale_tokens_atomically() {
    let mut v = validator();
    commit_native(&mut v, snapshot(100, 1_000_000_000, 100, 1));
    let before = state(&v);
    assert_eq!(
        v.prepare_rebind(0, point(4, 500)).unwrap_err(),
        DisciplineError::InvalidEpoch
    );
    assert_eq!(state(&v), before);
    let rebind = v.prepare_rebind(1, point(4, 500)).unwrap();
    assert_eq!(state(&v), before);
    commit_native(&mut v, snapshot(200, 1_000_000_000, 200, 2));
    let before = state(&v);
    assert_eq!(
        v.commit_rebind(rebind),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(state(&v), before);
    let rebind = v.prepare_rebind(1, point(4, 500)).unwrap();
    v.commit_rebind(rebind).unwrap();
    assert_eq!(v.epoch(), 1);
    assert_eq!(v.output_origin(), point(4, 500));
    assert_eq!(v.host_domain(), ClockDomainId(1));
    assert!(v.latest_record().is_none());
    assert!(v.latest_pair().is_none());
    let before = state(&v);
    assert_eq!(
        v.prepare_wasapi(0, snapshot(0, 0, 0, 0), None).unwrap_err(),
        DisciplineError::EpochMismatch
    );
    assert_eq!(state(&v), before);
    let new = ClockPair {
        source: point(4, 600),
        target: point(1, 300),
    };
    let prepared = v.prepare_pair(1, new).unwrap();
    v.commit(prepared).unwrap();
    assert_eq!(v.latest_pair(), Some(new));
}

#[test]
fn legacy_native_admission_remains_exactly_equivalent_to_pure_estimator_and_transport() {
    let config = DisciplineConfig::default();
    let mut legacy =
        PresentationDiscipline::new(config, point(2, 0), ClockDomainId(1), Timestamp::ZERO)
            .unwrap();
    let mut core =
        PresentationEstimator::new(config, point(2, 0), ClockDomainId(1), Timestamp::ZERO).unwrap();
    let mut legacy_transport = Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL);
    let mut core_transport = legacy_transport.clone();
    for second in 1..=4i64 {
        let output = second * 1_000_100_000;
        let host = second * 1_000_000_000;
        assert_eq!(
            legacy.observe(snapshot(
                output as u64,
                1_000_000_000,
                host,
                host as u64 / 100
            )),
            core.observe_progress_pair(pair(output, host))
                .map_err(DisciplineError::from)
        );
        assert_eq!(legacy.latest_pair(), core.latest_pair());
        assert_eq!(legacy.retained_len(), core.retained_len());
        let result = legacy.update(point(1, host), &mut legacy_transport);
        assert_eq!(
            result,
            core.update(point(1, host), &mut core_transport)
                .map_err(DisciplineError::from)
        );
        if second == 1 {
            assert_eq!(result, Ok(DisciplineUpdate::Warmup { span_ns: 0 }));
        }
        if second == 2 {
            assert_eq!(
                result,
                Ok(DisciplineUpdate::Applied {
                    base_rate_ppm: 100,
                    correction_ppm: 20,
                    applied_rate_ppm: 120,
                    phase_error_ns: 200_000,
                    limited: false
                })
            );
        }
        assert_eq!(legacy_transport.anchors(), core_transport.anchors());
        assert_eq!(
            legacy_transport.position_at(Timestamp::from_nanos(host + 500_000_000)),
            core_transport.position_at(Timestamp::from_nanos(host + 500_000_000))
        );
    }
    assert_eq!(legacy.quality(), ClockMappingQuality::Unknown);
}

#[test]
fn supplied_pairs_preserve_duplicate_semantics_and_reject_equal_host_progress() {
    let mut v = validator();
    let first = v.prepare_pair(0, pair(100, 100)).unwrap();
    assert_eq!(
        first.evidence(),
        &OriginalNativePresentationEvidence::SuppliedPair(pair(100, 100))
    );
    v.commit(first).unwrap();
    let before = state(&v);
    for received in [pair(100, 100), pair(100, 200)] {
        let prepared = v.prepare_pair(0, received).unwrap();
        assert_eq!(prepared.admission(), NativeObservationAdmission::Unchanged);
        assert_eq!(prepared.correlation_pair(), None);
        v.commit(prepared).unwrap();
        assert_eq!(state(&v), before);
    }
    for received in [pair(200, 100), pair(99, 200), pair(200, 99)] {
        assert_eq!(
            v.prepare_pair(0, received).unwrap_err(),
            DisciplineError::NonIncreasing
        );
        assert_eq!(state(&v), before);
    }
    let prepared = v.prepare_pair(0, pair(200, 200)).unwrap();
    assert_eq!(prepared.correlation_pair(), Some(pair(200, 200)));
    v.commit(prepared).unwrap();
    assert_eq!(v.latest_pair(), Some(pair(200, 200)));
}

#[test]
fn asio_tokens_bind_full_bracket_even_when_midpoint_and_render_are_identical() {
    let mut a = validator();
    let mut b = validator();
    let first_a = a.prepare_asio(0, asio(0, 1000, 90, 110), None).unwrap();
    a.commit(first_a).unwrap();
    let first_b = b.prepare_asio(0, asio(0, 1000, 95, 105), None).unwrap();
    b.commit(first_b).unwrap();
    assert_eq!(a.latest_pair(), b.latest_pair());
    assert_ne!(a.latest_record(), b.latest_record());
    let next = a.prepare_asio(0, asio(16, 1000, 190, 210), None).unwrap();
    let before = state(&b);
    assert_eq!(
        b.commit(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(state(&b), before);
    let next = a.prepare_rebind(1, point(4, 1000)).unwrap();
    let before = state(&b);
    assert_eq!(
        b.commit_rebind(next),
        Err(NativePreparationError::StalePreparation)
    );
    assert_eq!(state(&b), before);
    let basis = OutputFrameBasis::new(point(2, 0), 1000, 0).unwrap();
    assert_eq!(
        a.prepare_asio(0, asio(16, 1000, 190, 210), Some(basis))
            .unwrap_err(),
        DisciplineError::ObservationSourceChanged
    );
    let mut unavailable = snapshot(0, 0, 0, 0);
    unavailable.telemetry_available = false;
    assert_eq!(
        a.prepare_wasapi(0, unavailable, None).unwrap_err(),
        DisciplineError::Presentation(PresentationError::Unavailable)
    );
    assert_eq!(
        a.prepare_wasapi(0, snapshot(100, 1_000_000_000, 200, 2), None)
            .unwrap_err(),
        DisciplineError::ObservationSourceChanged
    );
}
