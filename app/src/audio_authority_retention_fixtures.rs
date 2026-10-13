//! Future-input pins are exact mathematical bounds, never permission to retire controls.
use super::*;
use beatkernel::audio::{OutputFrameBasis, TargetFrameBasis, TargetTime};
use beatkernel::input::{
    ButtonEvent, ButtonState, DeviceId, EventMeta, PhysicalControlId, PhysicalInputEvent,
};
use std::cmp::Ordering;

fn point(domain: u32, ns: i64) -> ClockPoint {
    ClockPoint {
        domain: ClockDomainId(domain),
        timestamp: Timestamp::from_nanos(ns),
    }
}
fn host(ns: i64) -> ClockPoint {
    point(1, ns)
}
fn bases() -> [AudioAuthorityCoordinateBasis; 3] {
    let source = OutputFrameBasis::new(point(2, 100), 3, 2).unwrap();
    [
        AudioAuthorityCoordinateBasis::SourceFrame(source),
        AudioAuthorityCoordinateBasis::TargetFrame(
            TargetFrameBasis::new(point(2, 100), TargetTime::new(0, 1, 3).unwrap(), 7).unwrap(),
        ),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: source,
            frequency: 4_000_000_000,
        },
    ]
}
fn descriptor(basis: AudioAuthorityCoordinateBasis) -> (TargetTime, u64) {
    match basis {
        AudioAuthorityCoordinateBasis::SourceFrame(b) => (
            TargetTime::from_frames(b.start_physical_frame(), b.sample_rate()).unwrap(),
            u64::from(b.sample_rate()),
        ),
        AudioAuthorityCoordinateBasis::SourceCounter {
            basis: b,
            frequency,
        } => (
            TargetTime::from_frames(b.start_physical_frame(), b.sample_rate()).unwrap(),
            frequency,
        ),
        AudioAuthorityCoordinateBasis::TargetFrame(b) => {
            (b.start_time(), u64::from(b.sample_rate()))
        }
    }
}
fn expected(basis: AudioAuthorityCoordinateBasis, units: u64) -> InterpolatedTargetTime {
    let (base, frequency) = descriptor(basis);
    InterpolatedTargetTime::new(base, units, units, frequency, 0, 1).unwrap()
}
fn equal(actual: InterpolatedTargetTime, expected: InterpolatedTargetTime) {
    assert_eq!(actual.compare_interpolated_time(expected), Ordering::Equal);
}
fn bounded(before: i64) -> ExtrapolationPolicy {
    ExtrapolationPolicy::Bounded {
        before: Duration::from_nanos(before),
        after: Duration::from_nanos(20),
    }
}
fn owner(
    basis: AudioAuthorityCoordinateBasis,
    policy: ExtrapolationPolicy,
    capacity: usize,
) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        AudioAuthorityConfig {
            history_capacity: capacity,
            max_observation_age: Duration::from_nanos(1_000),
            input_extrapolation: policy,
            max_input_ahead: Duration::from_nanos(i64::MAX),
        },
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap()
}
fn observe(owner: &mut AudioAuthority, units: u64, at: i64) {
    let basis = owner.coordinate_basis().unwrap();
    let coordinate = match basis {
        AudioAuthorityCoordinateBasis::SourceCounter { frequency, .. } => {
            NativeOutputCoordinate::Counter {
                position: units,
                frequency,
            }
        }
        _ => NativeOutputCoordinate::StreamFrame { frame: units },
    };
    assert_eq!(
        owner.observe_coordinate(
            7,
            ClockPair {
                source: basis.point(coordinate).unwrap(),
                target: host(at)
            },
            coordinate
        ),
        Ok(ObservationAdmission::Retained)
    );
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(0), vec![DeviceId(9)], 8).unwrap()
}
fn event(at: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(at), at as u64),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}
fn close(owner: &mut AudioAuthority, merger: &mut InputMerger, at: i64, now: i64) {
    owner.record_acquired_prefix(host(at)).unwrap();
    let token = owner
        .prepare_coordinate_frontier(host(now), merger)
        .unwrap()
        .unwrap();
    owner
        .commit_coordinate_frontier(token, host(now), merger)
        .unwrap();
    assert_eq!(owner.closed_host_prefix(), Some(host(at)));
}
fn assert_readonly(owner: &AudioAuthority, expected_query: InterpolatedTargetTime) {
    let state = owner.state();
    let entries = owner.history.clone();
    let capacity = owner.history.capacity();
    let slices = owner.history.as_slices();
    let storage = (slices.0.as_ptr(), slices.1.as_ptr());
    for _ in 0..128 {
        equal(
            owner.future_input_retention_query().unwrap(),
            expected_query,
        );
    }
    assert_eq!(owner.state(), state);
    assert_eq!(owner.history, entries);
    assert_eq!(owner.history.capacity(), capacity);
    let slices = owner.history.as_slices();
    assert_eq!((slices.0.as_ptr(), slices.1.as_ptr()), storage);
}

#[test]
fn cold_coordinate_owners_keep_fractional_creation_for_every_policy() {
    for basis in bases() {
        for policy in [ExtrapolationPolicy::Forbid, bounded(0), bounded(5)] {
            let owner = owner(basis, policy, 8);
            assert_readonly(&owner, expected(basis, 0));
            assert_eq!(owner.history_len(), 0);
            assert_eq!(owner.latest_coordinate_observation(), None);
        }
    }
}

#[test]
fn scalar_owner_refuses_coordinate_pin_without_mutation() {
    let coordinate = owner(bases()[0], ExtrapolationPolicy::Forbid, 8);
    let scalar = AudioAuthority::new(coordinate.config(), coordinate.epoch()).unwrap();
    let state = scalar.state();
    assert_eq!(
        scalar.future_input_retention_query().unwrap_err(),
        AudioAuthorityError::CoordinateModeMismatch
    );
    assert_eq!(scalar.state(), state);
    assert_eq!(scalar.history_len(), 0);
}

#[test]
fn forbid_without_closed_prefix_uses_first_original_anchor_not_latest_or_rounded_time() {
    for basis in bases() {
        let mut owner = owner(basis, ExtrapolationPolicy::Forbid, 8);
        observe(&mut owner, 2, 100);
        assert_readonly(&owner, expected(basis, 2));
        observe(&mut owner, 5, 110);
        assert_readonly(&owner, expected(basis, 2));
        let original = owner.history.front().unwrap();
        assert_eq!(original.pair.target, host(100));
        assert_eq!(CoordinateInputMapper::units(*original).unwrap(), 2);
        assert!(owner.closed_host_prefix().is_none());
        assert_eq!(owner.history_len(), 2);
    }
}

#[test]
fn bounded_before_preserves_creation_and_legal_before_first_input() {
    for basis in bases() {
        let mut owner = owner(basis, bounded(5), 8);
        observe(&mut owner, 2, 100);
        observe(&mut owner, 5, 110);
        owner.record_acquired_prefix(host(110)).unwrap();
        let prepared = owner
            .prepare_coordinate_input(host(95), host(110))
            .unwrap()
            .unwrap();
        assert!(!prepared.is_before_origin());
        let pin = owner.future_input_retention_query().unwrap();
        equal(pin, expected(basis, 0));
        assert_eq!(
            prepared.query().compare_interpolated_time(pin),
            Ordering::Greater
        );
        assert_eq!(
            prepared
                .query()
                .compare_interpolated_time(expected(basis, 2)),
            Ordering::Less
        );
        assert_readonly(&owner, pin);
        owner
            .validate_coordinate_input(&prepared, host(110))
            .unwrap();
        owner.commit_coordinate_input(prepared, host(110)).unwrap();
    }
}

#[test]
fn zero_bounded_before_keeps_creation_without_inventing_extrapolation_permission() {
    for basis in bases() {
        let mut owner = owner(basis, bounded(0), 8);
        observe(&mut owner, 2, 100);
        observe(&mut owner, 5, 110);
        owner.record_acquired_prefix(host(110)).unwrap();
        assert_readonly(&owner, expected(basis, 0));
        assert_eq!(
            owner
                .prepare_coordinate_input(host(99), host(110))
                .unwrap_err(),
            AudioAuthorityError::HistoryExpired
        );
        let token = owner
            .prepare_coordinate_input(host(100), host(110))
            .unwrap()
            .unwrap();
        assert_eq!(
            token
                .query()
                .compare_interpolated_time(owner.future_input_retention_query().unwrap()),
            Ordering::Greater
        );
    }
}

#[test]
fn closed_prefix_selects_covering_original_anchor_and_refuses_closed_inputs() {
    for basis in bases() {
        for (closed, units) in [(100, 2), (105, 2), (110, 5), (115, 5), (120, 9), (125, 9)] {
            let mut owner = owner(basis, bounded(5), 8);
            observe(&mut owner, 2, 100);
            observe(&mut owner, 5, 110);
            observe(&mut owner, 9, 120);
            let mut merger = merger();
            close(&mut owner, &mut merger, closed, 130);
            assert_readonly(&owner, expected(basis, units));
            assert_eq!(
                owner
                    .prepare_coordinate_input(host(closed), host(130))
                    .unwrap_err(),
                AudioAuthorityError::OperationRegression
            );
            assert!(matches!(
                merger.admit(event(closed), host(130)),
                Err(MergeError::LateInput { .. })
            ));
            owner.record_acquired_prefix(host(130)).unwrap();
            let next = closed + 1;
            let pending = event(next);
            merger.admit(pending.clone(), host(130)).unwrap();
            let prepared = owner
                .prepare_coordinate_input(host(next), host(130))
                .unwrap()
                .unwrap();
            assert!(!prepared.is_before_origin());
            assert_ne!(
                prepared
                    .query()
                    .compare_interpolated_time(owner.future_input_retention_query().unwrap()),
                Ordering::Less
            );
            assert_eq!(merger.peek_ready(host(130)).unwrap(), Some(&pending));
            owner.commit_coordinate_input(prepared, host(130)).unwrap();
            assert_eq!(merger.pop_ready(host(130)).unwrap(), Some(pending));
        }
    }
}

#[test]
fn real_correlation_restart_with_closed_before_first_preserves_policy_fallback() {
    for basis in bases() {
        for policy in [ExtrapolationPolicy::Forbid, bounded(5)] {
            let mut owner = owner(basis, policy, 8);
            observe(&mut owner, 0, 10);
            observe(&mut owner, 1, 20);
            let mut merger = merger();
            close(&mut owner, &mut merger, 20, 20);
            let restart = owner.prepare_correlation_restart(&merger).unwrap();
            owner.commit_correlation_restart(restart, &merger).unwrap();
            assert_readonly(&owner, expected(basis, 0));
            assert_eq!(owner.closed_host_prefix(), Some(host(20)));
            observe(&mut owner, 2, 30);
            observe(&mut owner, 3, 40);
            let minimum = if policy == ExtrapolationPolicy::Forbid {
                2
            } else {
                0
            };
            assert_readonly(&owner, expected(basis, minimum));
            owner.record_acquired_prefix(host(40)).unwrap();
            let at = if policy == ExtrapolationPolicy::Forbid {
                30
            } else {
                25
            };
            let token = owner
                .prepare_coordinate_input(host(at), host(40))
                .unwrap()
                .unwrap();
            assert_ne!(
                token
                    .query()
                    .compare_interpolated_time(owner.future_input_retention_query().unwrap()),
                Ordering::Less
            );
            owner.commit_coordinate_input(token, host(40)).unwrap();
        }
    }
}

#[test]
fn repeated_readonly_pin_queries_preserve_real_prepared_input_and_frontier_commits() {
    for basis in bases() {
        for input_commit in [true, false] {
            let mut owner = owner(basis, ExtrapolationPolicy::Forbid, 8);
            observe(&mut owner, 2, 100);
            observe(&mut owner, 5, 110);
            owner.record_acquired_prefix(host(110)).unwrap();
            let mut merger = merger();
            if input_commit {
                let token = owner
                    .prepare_coordinate_input(host(105), host(110))
                    .unwrap()
                    .unwrap();
                assert_readonly(&owner, expected(basis, 2));
                owner.validate_coordinate_input(&token, host(110)).unwrap();
                owner.commit_coordinate_input(token, host(110)).unwrap();
                equal(
                    owner.committed_coordinate_operation().unwrap(),
                    token.query(),
                );
            } else {
                let token = owner
                    .prepare_coordinate_frontier(host(110), &merger)
                    .unwrap()
                    .unwrap();
                assert_readonly(&owner, expected(basis, 2));
                owner
                    .commit_coordinate_frontier(token, host(110), &mut merger)
                    .unwrap();
                equal(
                    owner.committed_coordinate_presentation().unwrap(),
                    token.query(),
                );
                assert_eq!(owner.closed_host_prefix(), Some(host(110)));
                assert_readonly(&owner, expected(basis, 5));
            }
        }
    }
}

#[test]
fn original_counter_units_above_nanosecond_precision_survive_pin_selection() {
    let basis = AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 0), 3, 0).unwrap(),
        frequency: u64::MAX,
    };
    let mut owner = owner(basis, ExtrapolationPolicy::Forbid, 8);
    observe(&mut owner, 1, 100);
    observe(&mut owner, 2, 110);
    assert_eq!(owner.history[0].pair.source, owner.history[1].pair.source);
    assert_readonly(&owner, expected(basis, 1));
    let mut merger = merger();
    close(&mut owner, &mut merger, 110, 110);
    assert_readonly(&owner, expected(basis, 2));
    assert_eq!(
        owner
            .future_input_retention_query()
            .unwrap()
            .compare_interpolated_time(expected(basis, 1)),
        Ordering::Greater
    );
}

#[test]
fn capacity_retirement_keeps_existing_expired_historical_control_behavior() {
    let basis = bases()[2];
    let mut owner = owner(basis, ExtrapolationPolicy::Forbid, 3);
    observe(&mut owner, 0, 100);
    observe(&mut owner, 2, 110);
    let mut merger = merger();
    close(&mut owner, &mut merger, 110, 110);
    observe(&mut owner, 3, 120);
    observe(&mut owner, 4, 130);
    assert_eq!(owner.history_len(), 3);
    assert_eq!(
        CoordinateInputMapper::units(*owner.history.front().unwrap()).unwrap(),
        2
    );
    assert_readonly(&owner, expected(basis, 2));
    let state = owner.state();
    assert_eq!(
        owner.presented_coordinate_boundary_host(&expected(basis, 1), host(130)),
        Err(AudioAuthorityError::HistoryExpired)
    );
    assert_eq!(owner.state(), state);
}

#[test]
fn before_creation_inputs_keep_discard_semantics_without_native_playback_query_permission() {
    for basis in bases() {
        let mut owner = owner(basis, bounded(5), 8);
        observe(&mut owner, 0, 100);
        observe(&mut owner, 1, 110);
        owner.record_acquired_prefix(host(110)).unwrap();
        let token = owner
            .prepare_coordinate_input(host(95), host(110))
            .unwrap()
            .unwrap();
        assert!(token.is_before_origin());
        assert_eq!(
            token
                .query()
                .compare_interpolated_time(owner.future_input_retention_query().unwrap()),
            Ordering::Less
        );
        assert_readonly(&owner, expected(basis, 0));
        let before = owner.state();
        assert_eq!(
            owner.commit_coordinate_input(token, host(110)),
            Err(AudioAuthorityError::OperationRegression)
        );
        assert_eq!(owner.state(), before);
        owner
            .commit_discarded_coordinate_input(token, host(110))
            .unwrap();
        assert_eq!(owner.committed_input_host(), Some(host(95)));
        assert!(owner.committed_coordinate_operation().is_none());
        assert!(owner.committed_coordinate_presentation().is_none());
        assert_readonly(&owner, expected(basis, 0));
    }
}
