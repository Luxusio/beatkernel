//! Original-coordinate storage is metadata, not permission to map or advance gameplay.
use super::*;
use crate::native_output_coordinate::NativeOutputCoordinate;
use beatkernel::audio::{OutputFrameBasis, TargetFrameBasis, TargetTime};
use beatkernel::input::{
    ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
};

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn epoch(origin: i64) -> AudioAuthorityEpoch {
    AudioAuthorityEpoch {
        id: 7,
        stream_origin: point(2, origin),
        logical_origin: point(3, 500),
        host_domain: ClockDomainId(1),
    }
}
fn config(capacity: usize) -> AudioAuthorityConfig {
    AudioAuthorityConfig {
        history_capacity: capacity,
        max_observation_age: Duration::from_nanos(10),
        ..AudioAuthorityConfig::default()
    }
}
fn source_basis() -> OutputFrameBasis {
    OutputFrameBasis::new(point(2, 100), 2_000_000_000, 0).unwrap()
}
fn counter(position: u64) -> NativeOutputCoordinate {
    NativeOutputCoordinate::Counter {
        position,
        frequency: 2_000_000_000,
    }
}
fn frame(frame: u64) -> NativeOutputCoordinate {
    NativeOutputCoordinate::StreamFrame { frame }
}
fn pair(ns: i64, at: i64) -> ClockPair {
    ClockPair {
        source: point(2, ns),
        target: host(at),
    }
}
fn owner(capacity: usize) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        config(capacity),
        epoch(100),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: source_basis(),
            frequency: 2_000_000_000,
        },
    )
    .unwrap()
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn unchanged(
    owner: &mut AudioAuthority,
    operation: impl FnOnce(&mut AudioAuthority) -> Result<ObservationAdmission, AudioAuthorityError>,
    error: AudioAuthorityError,
) {
    let state = owner.state();
    let entries = owner.history.clone();
    assert_eq!(operation(owner), Err(error));
    assert_eq!(owner.state(), state);
    assert_eq!(owner.history, entries);
}

#[test]
fn counter_progress_retains_original_units_when_nanoseconds_are_flat() {
    let mut owner = owner(4);
    assert_eq!(
        owner.observe_coordinate(7, pair(100, 100), counter(0)),
        Ok(ObservationAdmission::Retained)
    );
    assert_eq!(
        owner.observe_coordinate(7, pair(100, 101), counter(1)),
        Ok(ObservationAdmission::Retained)
    );
    assert_eq!(owner.history_len(), 2);
    let latest = owner.latest_coordinate_observation().unwrap();
    assert_eq!(latest.pair(), pair(100, 101));
    assert_eq!(latest.coordinate(), Some(counter(1)));
    assert_eq!(owner.latest_observation(), Some(pair(100, 101)));
    assert!(owner.has_fresh_observation(host(111)).unwrap());
    assert!(!owner.has_fresh_observation(host(112)).unwrap());
}

#[test]
fn source_and_target_frame_progress_preserve_flat_nanoseconds() {
    let target = TargetFrameBasis::new(
        point(2, 100),
        TargetTime::from_frames(0, 2_000_000_000).unwrap(),
        2_000_000_000,
    )
    .unwrap();
    for basis in [
        AudioAuthorityCoordinateBasis::SourceFrame(source_basis()),
        AudioAuthorityCoordinateBasis::TargetFrame(target),
    ] {
        let mut owner = AudioAuthority::new_with_coordinates(config(4), epoch(100), basis).unwrap();
        for (position, at) in [(0, 100), (1, 101)] {
            assert_eq!(
                owner.observe_coordinate(7, pair(100, at), frame(position)),
                Ok(ObservationAdmission::Retained)
            );
        }
        assert_eq!(
            owner.latest_coordinate_observation().unwrap().coordinate(),
            Some(frame(1))
        );
        assert_eq!(owner.coordinate_basis(), Some(basis));
    }
}

#[test]
fn unchanged_coordinate_never_refreshes_pair_revision_or_age() {
    let mut owner = owner(4);
    owner
        .observe_coordinate(7, pair(100, 100), counter(0))
        .unwrap();
    let before = owner.state();
    let entries = owner.history.clone();
    for at in [100, 101, 10_000] {
        assert_eq!(
            owner.observe_coordinate(7, pair(100, at), counter(0)),
            Ok(ObservationAdmission::Unchanged)
        );
        assert_eq!(owner.state(), before);
        assert_eq!(owner.history, entries);
    }
    assert!(!owner.has_fresh_observation(host(111)).unwrap());
}

#[test]
fn invalid_observations_preserve_every_entry_and_watermark() {
    let mut owner = owner(8);
    owner
        .observe_coordinate(7, pair(100, 100), counter(0))
        .unwrap();
    owner
        .observe_coordinate(7, pair(101, 101), counter(2))
        .unwrap();
    owner.record_acquired_prefix(host(101)).unwrap();
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(8, pair(101, 102), counter(3)),
        AudioAuthorityError::WrongEpoch,
    );
    unchanged(
        &mut owner,
        |o| {
            o.observe_coordinate(
                7,
                ClockPair {
                    source: point(4, 101),
                    target: host(102),
                },
                counter(3),
            )
        },
        AudioAuthorityError::DomainMismatch,
    );
    unchanged(
        &mut owner,
        |o| {
            o.observe_coordinate(
                7,
                ClockPair {
                    source: point(2, 101),
                    target: point(4, 102),
                },
                counter(3),
            )
        },
        AudioAuthorityError::DomainMismatch,
    );
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(101, 102), frame(3)),
        AudioAuthorityError::CoordinateMismatch,
    );
    unchanged(
        &mut owner,
        |o| {
            o.observe_coordinate(
                7,
                pair(101, 102),
                NativeOutputCoordinate::Counter {
                    position: 3,
                    frequency: 1,
                },
            )
        },
        AudioAuthorityError::CoordinateMismatch,
    );
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(102, 102), counter(3)),
        AudioAuthorityError::CoordinateMismatch,
    );
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(100, 102), counter(1)),
        AudioAuthorityError::ObservationRegression,
    );
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(101, 100), counter(3)),
        AudioAuthorityError::ObservationRegression,
    );
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(101, 101), counter(3)),
        AudioAuthorityError::ObservationRegression,
    );
}

#[test]
fn scalar_and_coordinate_modes_cannot_mix() {
    let mut opted = owner(4);
    unchanged(
        &mut opted,
        |o| o.observe(7, pair(100, 100)),
        AudioAuthorityError::CoordinateModeMismatch,
    );
    let mut scalar = AudioAuthority::new(config(4), epoch(100)).unwrap();
    unchanged(
        &mut scalar,
        |o| o.observe_coordinate(7, pair(100, 100), counter(0)),
        AudioAuthorityError::CoordinateModeMismatch,
    );
    scalar.observe(7, pair(100, 100)).unwrap();
    assert_eq!(scalar.coordinate_basis(), None);
    assert_eq!(scalar.latest_coordinate_observation(), None);
    assert_eq!(scalar.latest_observation(), Some(pair(100, 100)));
}

#[test]
fn full_coordinate_history_cannot_retire_without_real_closed_prefix() {
    let mut owner = owner(2);
    owner
        .observe_coordinate(7, pair(100, 100), counter(0))
        .unwrap();
    owner
        .observe_coordinate(7, pair(100, 101), counter(1))
        .unwrap();
    owner.record_acquired_prefix(host(102)).unwrap();
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(101, 102), counter(2)),
        AudioAuthorityError::HistoryCapacity,
    );
    assert_eq!(owner.closed_host_prefix(), None);
}

#[test]
fn original_nonzero_source_basis_combines_fraction_before_one_floor() {
    // Absolute start 2/3s plus one counter tick 1/3s = exactly 1s.
    // Independently floored offsets would sum to 999999999ns.
    let basis = OutputFrameBasis::new(point(2, 100), 3, 2).unwrap();
    let descriptor = AudioAuthorityCoordinateBasis::SourceCounter {
        basis,
        frequency: 3,
    };
    let mut owner =
        AudioAuthority::new_with_coordinates(config(4), epoch(666_666_766), descriptor).unwrap();
    let coord = NativeOutputCoordinate::Counter {
        position: 1,
        frequency: 3,
    };
    assert_eq!(
        owner.observe_coordinate(7, pair(1_000_000_100, 100), coord),
        Ok(ObservationAdmission::Retained)
    );
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(1_000_000_099, 101), coord),
        AudioAuthorityError::CoordinateMismatch,
    );
}

#[test]
fn original_fractional_target_start_is_added_exactly_once() {
    let basis = TargetFrameBasis::new(point(2, 100), TargetTime::new(0, 1, 3).unwrap(), 3).unwrap();
    let descriptor = AudioAuthorityCoordinateBasis::TargetFrame(basis);
    let mut owner =
        AudioAuthority::new_with_coordinates(config(4), epoch(333_333_433), descriptor).unwrap();
    assert_eq!(
        owner.observe_coordinate(7, pair(666_666_766, 100), frame(1)),
        Ok(ObservationAdmission::Retained)
    );
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(666_666_765, 101), frame(1)),
        AudioAuthorityError::CoordinateMismatch,
    );
}

#[test]
fn coordinate_overflow_and_revision_exhaustion_refuse_before_mutation() {
    let basis = OutputFrameBasis::new(point(2, i64::MAX), 1, 0).unwrap();
    let mut overflowing = AudioAuthority::new_with_coordinates(
        config(4),
        epoch(i64::MAX),
        AudioAuthorityCoordinateBasis::SourceFrame(basis),
    )
    .unwrap();
    unchanged(
        &mut overflowing,
        |o| o.observe_coordinate(7, pair(i64::MAX, 100), frame(1)),
        AudioAuthorityError::Overflow,
    );
    let mut owner = owner(4);
    owner
        .observe_coordinate(7, pair(100, 100), counter(0))
        .unwrap();
    owner.revision = u64::MAX;
    unchanged(
        &mut owner,
        |o| o.observe_coordinate(7, pair(100, 101), counter(1)),
        AudioAuthorityError::Overflow,
    );
    assert_eq!(
        owner.observe_coordinate(7, pair(100, 102), counter(0)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(owner.revision, u64::MAX);
}

#[test]
fn cold_descriptor_refusals_do_not_create_an_owner() {
    assert!(matches!(
        AudioAuthority::new_with_coordinates(
            config(4),
            epoch(101),
            AudioAuthorityCoordinateBasis::SourceFrame(source_basis())
        ),
        Err(AudioAuthorityError::CoordinateMismatch)
    ));
    let overflow = OutputFrameBasis::new(point(2, i64::MAX), 1, 1).unwrap();
    assert!(matches!(
        AudioAuthority::new_with_coordinates(
            config(4),
            epoch(i64::MAX),
            AudioAuthorityCoordinateBasis::SourceFrame(overflow)
        ),
        Err(AudioAuthorityError::Overflow)
    ));
    assert!(matches!(
        AudioAuthority::new_with_coordinates(
            config(4),
            epoch(100),
            AudioAuthorityCoordinateBasis::SourceCounter {
                basis: source_basis(),
                frequency: 0
            }
        ),
        Err(AudioAuthorityError::InvalidConfig)
    ));
    let wrong_domain = OutputFrameBasis::new(point(4, 100), 2_000_000_000, 0).unwrap();
    assert!(matches!(
        AudioAuthority::new_with_coordinates(
            config(4),
            epoch(100),
            AudioAuthorityCoordinateBasis::SourceFrame(wrong_domain)
        ),
        Err(AudioAuthorityError::DomainMismatch)
    ));
    for capacity in [0, 1, 1025] {
        assert!(matches!(
            AudioAuthority::new_with_coordinates(
                config(capacity),
                epoch(100),
                AudioAuthorityCoordinateBasis::SourceFrame(source_basis())
            ),
            Err(AudioAuthorityError::InvalidConfig)
        ));
    }
}

#[test]
fn every_scalar_mapping_boundary_fails_closed_for_coordinate_owner() {
    let mut owner = owner(4);
    let merger = merger();
    owner
        .observe_coordinate(7, pair(100, 100), counter(0))
        .unwrap();
    owner
        .observe_coordinate(7, pair(101, 102), counter(2))
        .unwrap();
    owner.record_acquired_prefix(host(102)).unwrap();
    let before = owner.state();
    let error = AudioAuthorityError::CoordinateMappingUnavailable;
    assert_eq!(owner.prepare_input(host(101), host(102)), Err(error));
    assert_eq!(owner.prepare_frontier(host(102), &merger), Err(error));
    assert_eq!(owner.prepare_held_frontier(host(102), &merger), Err(error));
    assert_eq!(
        owner.presented_boundary_host(point(2, 101), host(102)),
        Err(error)
    );
    assert_eq!(
        owner.prepare_control_cutoff(7, point(2, 101), host(101), host(102), &merger),
        Err(error)
    );
    assert_eq!(
        owner.prepare_resume_control_cutoff(7, point(2, 101), host(101), host(102), &merger),
        Err(error)
    );
    assert_eq!(owner.checked_logical_output(point(2, 101)), Err(error));
    let next = AudioAuthorityEpoch {
        id: 8,
        ..epoch(100)
    };
    assert_eq!(owner.prepare_epoch(next, &merger), Err(error));
    let restart = owner.prepare_correlation_restart(&merger).unwrap();
    assert_eq!(
        owner.prepare_primed_restart(
            restart,
            [pair(100, 100), pair(101, 102)],
            host(102),
            &merger
        ),
        Err(error)
    );
    assert_eq!(owner.state(), before);
}

#[test]
fn restart_preserves_descriptor_and_all_gameplay_watermarks() {
    let mut owner = owner(4);
    let merger = merger();
    owner
        .observe_coordinate(7, pair(100, 100), counter(0))
        .unwrap();
    owner.record_acquired_prefix(host(100)).unwrap();
    // AC029 cannot produce new gameplay cuts. These preexisting owner fields
    // model retained accepted state; they do not authorize observation retirement.
    owner.closed = Some(host(90));
    owner.input_host = Some(host(80));
    owner.operation = Some(point(3, 500));
    owner.presentation = Some(point(3, 500));
    let before = owner.state();
    let restart = owner.prepare_correlation_restart(&merger).unwrap();
    owner.commit_correlation_restart(restart, &merger).unwrap();
    let after = owner.state();
    assert_eq!(after.coordinate_basis, before.coordinate_basis);
    assert_eq!(after.acquired, before.acquired);
    assert_eq!(after.closed, before.closed);
    assert_eq!(after.input_host, before.input_host);
    assert_eq!(after.operation, before.operation);
    assert_eq!(after.presentation, before.presentation);
    assert_eq!(owner.history_len(), 0);
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(
        owner.commit_correlation_restart(restart, &merger),
        Err(AudioAuthorityError::StalePreparation)
    );
}

#[test]
fn real_pending_input_refuses_restart_without_changing_owner() {
    let owner = owner(4);
    let mut merger = merger();
    merger
        .admit(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(DeviceId(9), host(100), 1),
                control: PhysicalControlId::keyboard(7u16),
                state: ButtonState::Down,
            }),
            host(100),
        )
        .unwrap();
    let before = owner.state();
    assert_eq!(
        owner.prepare_correlation_restart(&merger),
        Err(AudioAuthorityError::PendingInputs)
    );
    assert_eq!(owner.state(), before);
    assert_eq!(merger.pending(), 1);
}

#[test]
fn descriptorless_primed_tokens_cannot_be_validated_or_committed_into_opted_owner() {
    let scalar = AudioAuthority::new(config(4), epoch(100)).unwrap();
    let merger = merger();
    let restart = scalar.prepare_correlation_restart(&merger).unwrap();
    let prepared = scalar
        .prepare_primed_restart(
            restart,
            [pair(100, 100), pair(102, 102)],
            host(102),
            &merger,
        )
        .unwrap();
    let next = AudioAuthorityEpoch {
        id: 8,
        ..epoch(100)
    };
    let epoch_token = scalar.prepare_epoch(next, &merger).unwrap();
    let mut opted = owner(4);
    let before = opted.state();
    let error = AudioAuthorityError::CoordinateMappingUnavailable;
    assert_eq!(
        opted.prepare_primed_epoch(
            epoch_token,
            [pair(100, 100), pair(102, 102)],
            host(102),
            &merger
        ),
        Err(error)
    );
    assert_eq!(
        opted.validate_primed_correlation(&prepared, host(102), &merger),
        Err(error)
    );
    assert_eq!(
        opted.commit_primed_correlation(prepared, host(102), &merger),
        Err(error)
    );
    assert_eq!(opted.state(), before);
    assert_eq!(opted.history_len(), 0);
}

#[test]
fn legacy_flat_scalar_observation_keeps_original_freshness_and_mapping() {
    let mut owner = AudioAuthority::new(config(4), epoch(100)).unwrap();
    owner.observe(7, pair(100, 100)).unwrap();
    let before = owner.state();
    assert_eq!(
        owner.observe(7, pair(100, 101)),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(owner.state(), before);
    assert!(!owner.has_fresh_observation(host(111)).unwrap());
    owner.observe(7, pair(102, 102)).unwrap();
    owner.record_acquired_prefix(host(102)).unwrap();
    assert_eq!(
        owner
            .prepare_input(host(101), host(102))
            .unwrap()
            .unwrap()
            .output(),
        point(3, 501)
    );
}
