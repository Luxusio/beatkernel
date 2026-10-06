//! Deferred original WASAPI metadata admission through the actual shared helper.
use super::*;
use beatkernel::{
    audio::OutputFrameBasis,
    time::{ClockDomainId, ClockPoint, ClockMappingQuality, Timestamp},
};
use beatkernel_platform::audio::{
    AudioClockSnapshot, AudioClockReadingQuality, AudioStreamSnapshot, AudioStreamStatus,
    StreamCounters,
    presentation::{
        PresentationError,
        discipline::{DisciplineConfig, DisciplineError, PresentationDiscipline},
    },
};
fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn basis() -> OutputFrameBasis {
    OutputFrameBasis::new(point(2, 0), 3, 2).unwrap()
}
fn observer(epoch: u64, basis: OutputFrameBasis) -> PresentationDiscipline {
    let origin = basis.point_at_stream_frame(0).unwrap();
    let mut p = PresentationDiscipline::new(
        DisciplineConfig::default(),
        origin,
        ClockDomainId(1),
        Timestamp::from_nanos(604_800_000_000_000),
    )
    .unwrap();
    if epoch != 0 {
        p.rebind_output(
            epoch,
            origin,
            origin,
            Timestamp::from_nanos(604_800_000_000_000),
        )
        .unwrap();
    }
    p
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
fn original_counter_evidence_keeps_rational_carry_basis_offset_and_max_epoch_without_retagging_old_snapshot()
 {
    let basis = basis();
    let mut p = observer(u64::MAX, basis);
    let before = format!("{p:?}");
    let mut old = snapshot(0, 0, 0);
    old.telemetry_available = false;
    old.status = AudioStreamStatus::Ready;
    assert!(matches!(
        observe_wasapi(&mut p, 0, old, basis),
        Err(ReplacementObservationError::EpochMismatch)
    ));
    assert_eq!(format!("{p:?}"), before);
    assert!(observe_wasapi(&mut p, u64::MAX, snapshot(1, 3, 1_000_000_000), basis).unwrap());
    assert_eq!(p.latest_pair().unwrap().source, point(2, 1_000_000_000));
    assert_eq!(p.latest_pair().unwrap().target, point(1, 1_000_000_000));
    assert_eq!(p.epoch(), u64::MAX);
}
#[test]
fn soft_wait_states_do_not_bind_source_or_invent_first_observation() {
    let basis = basis();
    for case in 0..5 {
        let mut p = observer(7, basis);
        let before = format!("{p:?}");
        let mut s = snapshot(1, 3, 1_000_000_000);
        match case {
            0 => s.status = AudioStreamStatus::Ready,
            1 => s.telemetry_available = false,
            2 => s.clock = None,
            3 => s.clock.as_mut().unwrap().position = 0,
            _ => s.clock.as_mut().unwrap().host_point = None,
        }
        assert!(!observe_wasapi(&mut p, 7, s, basis).unwrap());
        assert_eq!(format!("{p:?}"), before);
        assert_eq!(p.latest_pair(), None);
        assert_eq!(p.retained_len(), 0);
    }
}
#[test]
fn terminal_statuses_preserve_exact_failure_namespace_and_do_not_become_soft_waits() {
    let basis = basis();
    for status in [
        AudioStreamStatus::Stopped,
        AudioStreamStatus::Failed { hresult: i32::MIN },
        AudioStreamStatus::Failed { hresult: i32::MAX },
        AudioStreamStatus::WorkerPanicked,
    ] {
        for available in [false, true] {
            let mut p = observer(9, basis);
            observe_wasapi(&mut p, 9, snapshot(1, 3, 1_000_000_000), basis).unwrap();
            let before = format!("{p:?}");
            let mut s = snapshot(2, 3, 2_000_000_000);
            s.status = status;
            s.telemetry_available = available;
            match observe_wasapi(&mut p, 9, s, basis) {
                Err(ReplacementObservationError::Status(actual)) => assert_eq!(actual, status),
                _ => panic!("terminal status must remain explicit"),
            }
            assert_eq!(format!("{p:?}"), before);
        }
    }
}
#[test]
fn duplicates_are_valid_existing_evidence_but_do_not_refresh_age_and_real_progress_retains_original_pairs()
 {
    let basis = basis();
    let mut p = observer(7, basis);
    let first = snapshot(1, 3, 1_000_000_000);
    assert!(observe_wasapi(&mut p, 7, first, basis).unwrap());
    let before = format!("{p:?}");
    assert!(observe_wasapi(&mut p, 7, first, basis).unwrap());
    assert_eq!(format!("{p:?}"), before);
    assert_eq!(
        p.validate_host(point(1, 3_000_000_001)),
        Err(DisciplineError::Stale)
    );
    assert!(observe_wasapi(&mut p, 7, snapshot(2, 3, 1_333_333_333), basis).unwrap());
    assert_eq!(p.latest_pair().unwrap().source, point(2, 1_333_333_333));
    assert_eq!(p.retained_len(), 2);
}
#[test]
fn malformed_quality_frequency_domain_and_changed_grid_are_hard_atomic_refusals() {
    let basis = basis();
    let mut p = observer(7, basis);
    observe_wasapi(&mut p, 7, snapshot(1, 3, 1_000_000_000), basis).unwrap();
    let before = format!("{p:?}");
    let mut degraded = snapshot(2, 3, 2_000_000_000);
    degraded.clock.as_mut().unwrap().reading_quality = AudioClockReadingQuality::Degraded;
    assert!(matches!(
        observe_wasapi(&mut p, 7, degraded, basis),
        Err(ReplacementObservationError::Discipline(
            DisciplineError::Presentation(PresentationError::Inaccurate)
        ))
    ));
    assert_eq!(format!("{p:?}"), before);
    for s in [
        snapshot(2, 0, 2_000_000_000),
        snapshot(2, 4, 2_000_000_000),
        snapshot(0, 3, 2_000_000_000),
    ] {
        if s.clock.unwrap().position == 0 {
            assert!(!observe_wasapi(&mut p, 7, s, basis).unwrap());
        } else {
            assert!(observe_wasapi(&mut p, 7, s, basis).is_err());
        }
        assert_eq!(format!("{p:?}"), before);
    }
    let mut foreign = snapshot(2, 3, 2_000_000_000);
    foreign.clock.as_mut().unwrap().host_point = Some(point(9, 2_000_000_000));
    assert!(observe_wasapi(&mut p, 7, foreign, basis).is_err());
    assert_eq!(format!("{p:?}"), before);
    let equivalent_zero_changed_grid = OutputFrameBasis::new(point(2, 0), 6, 4).unwrap();
    assert!(matches!(
        observe_wasapi(
            &mut p,
            7,
            snapshot(2, 3, 2_000_000_000),
            equivalent_zero_changed_grid
        ),
        Err(ReplacementObservationError::Discipline(
            DisciplineError::ObservationSourceChanged
        ))
    ));
    assert_eq!(format!("{p:?}"), before);
}
#[test]
fn full_width_native_units_and_large_timestamps_remain_exact_and_checked_overflow_keeps_empty_owner()
 {
    let basis = OutputFrameBasis::new(point(2, 0), u32::MAX, u64::MAX).unwrap();
    let mut p = observer(u64::MAX, basis);
    assert!(
        observe_wasapi(
            &mut p,
            u64::MAX,
            snapshot(u64::MAX, u64::MAX, 9_007_199_254_740_993),
            basis
        )
        .unwrap()
    );
    assert_eq!(
        p.latest_pair().unwrap().source,
        point(2, 4_294_967_298_000_000_000)
    );
    assert_eq!(
        p.latest_pair().unwrap().target,
        point(1, 9_007_199_254_740_993)
    );
    let overflow_basis = OutputFrameBasis::new(point(2, i64::MAX), 3, 0).unwrap();
    let mut empty = observer(7, overflow_basis);
    let before = format!("{empty:?}");
    assert!(observe_wasapi(&mut empty, 7, snapshot(1, 3, 1_000_000_000), overflow_basis).is_err());
    assert_eq!(format!("{empty:?}"), before);
}
