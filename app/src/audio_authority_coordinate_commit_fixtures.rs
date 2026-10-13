//! Exact mathematical commit contracts; no native hardware or Runtime permission.
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
fn config() -> AudioAuthorityConfig {
    AudioAuthorityConfig {
        history_capacity: 8,
        max_observation_age: Duration::from_nanos(100),
        input_extrapolation: ExtrapolationPolicy::Bounded {
            before: Duration::from_nanos(2),
            after: Duration::from_nanos(8),
        },
        max_input_ahead: Duration::from_nanos(2),
    }
}
fn counter(frequency: u64) -> AudioAuthorityCoordinateBasis {
    AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 0), 3, 0).unwrap(),
        frequency,
    }
}
fn owner(basis: AudioAuthorityCoordinateBasis) -> AudioAuthority {
    AudioAuthority::new_with_coordinates(
        config(),
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
    owner
        .observe_coordinate(
            7,
            ClockPair {
                source: basis.point(coordinate).unwrap(),
                target: host(at),
            },
            coordinate,
        )
        .unwrap();
}
fn seeded(basis: AudioAuthorityCoordinateBasis) -> AudioAuthority {
    let mut result = owner(basis);
    observe(&mut result, 0, 100);
    observe(&mut result, 1, 101);
    result.record_acquired_prefix(host(105)).unwrap();
    result
}
fn input(owner: &AudioAuthority, at: i64, now: i64) -> PreparedCoordinateInput {
    owner
        .prepare_coordinate_input(host(at), host(now))
        .unwrap()
        .unwrap()
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
fn equal(left: InterpolatedTargetTime, right: InterpolatedTargetTime) {
    assert_eq!(left.compare_interpolated_time(right), Ordering::Equal);
}
fn snapshot(owner: &AudioAuthority) -> String {
    format!(
        "{:?}",
        (
            owner.state(),
            &owner.history,
            owner.committed_coordinate_operation(),
            owner.committed_coordinate_presentation()
        )
    )
}

#[test]
fn successful_input_commits_once_and_equal_host_can_commit_equal_exact_time() {
    let mut owner = seeded(counter(4_000_000_000));
    let token = input(&owner, 101, 105);
    let query = token.query();
    owner.commit_coordinate_input(token, host(106)).unwrap();
    assert_eq!(owner.committed_input_host(), Some(host(101)));
    assert_eq!(owner.committed_operation(), Some(point(3, 500)));
    equal(owner.committed_coordinate_operation().unwrap(), query);
    assert!(owner.committed_coordinate_presentation().is_none());
    let state = snapshot(&owner);
    assert_eq!(
        owner.commit_coordinate_input(token, host(106)),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(snapshot(&owner), state);
    let again = input(&owner, 101, 106);
    owner.commit_coordinate_input(again, host(106)).unwrap();
    equal(owner.committed_coordinate_operation().unwrap(), query);
}

#[test]
fn reanchor_cannot_regress_prediction_hidden_by_the_same_nanosecond() {
    let mut owner = seeded(counter(4_000_000_000));
    let prediction = input(&owner, 103, 105); // 3/4 ns.
    owner
        .commit_coordinate_input(prediction, host(105))
        .unwrap();
    observe(&mut owner, 2, 104); // 1/2 ns, still rounded to zero.
    let before = snapshot(&owner);
    assert_eq!(
        owner
            .prepare_coordinate_input(host(104), host(105))
            .unwrap_err(),
        AudioAuthorityError::OperationRegression
    );
    assert_eq!(owner.committed_operation(), Some(point(3, 500)));
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn exact_preorigin_can_share_the_rounded_origin_and_only_retire_host() {
    let basis = AudioAuthorityCoordinateBasis::SourceCounter {
        basis: OutputFrameBasis::new(point(2, 0), 3, 1).unwrap(),
        frequency: 3_000_000_000,
    };
    let mut owner = owner(basis);
    observe(&mut owner, 1, 100);
    observe(&mut owner, 2, 101);
    owner.record_acquired_prefix(host(102)).unwrap();
    let token = input(&owner, 98, 102); // 1/3 second - 1/(3e9) second.
    assert_eq!(token.raw_output(), owner.epoch().stream_origin);
    assert_eq!(token.output(), owner.epoch().logical_origin);
    assert!(token.is_before_origin());
    let before = snapshot(&owner);
    assert!(owner.commit_coordinate_input(token, host(102)).is_err());
    assert_eq!(snapshot(&owner), before);
    owner
        .commit_discarded_coordinate_input(token, host(102))
        .unwrap();
    assert_eq!(owner.committed_input_host(), Some(host(98)));
    assert_eq!(owner.committed_operation(), None);
    assert!(owner.committed_coordinate_operation().is_none());
    assert_eq!(owner.committed_presentation(), None);
    assert_eq!(owner.closed_host_prefix(), None);
}

#[test]
fn wrong_discard_classification_and_tampered_classification_are_atomic() {
    let mut owner = seeded(counter(4_000_000_000));
    let token = input(&owner, 101, 105);
    let before = snapshot(&owner);
    assert!(owner
        .commit_discarded_coordinate_input(token, host(105))
        .is_err());
    assert_eq!(snapshot(&owner), before);
    let mut tampered = token;
    tampered.before_origin = true;
    assert!(owner
        .commit_discarded_coordinate_input(tampered, host(105))
        .is_err());
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn input_commit_rechecks_freshness_time_domain_and_state() {
    for mutation in 0..4 {
        let mut owner = seeded(counter(4_000_000_000));
        let token = input(&owner, 101, 105);
        let now = match mutation {
            0 => host(202),
            1 => host(104),
            2 => point(4, 105),
            _ => {
                owner.record_acquired_prefix(host(106)).unwrap();
                host(106)
            }
        };
        let before = snapshot(&owner);
        assert!(owner.commit_coordinate_input(token, now).is_err());
        assert_eq!(snapshot(&owner), before);
    }
}

#[test]
fn changed_query_with_identical_rounded_output_is_not_a_valid_token() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut token = input(&owner, 101, 105);
    let before = snapshot(&owner);
    token.query = input(&owner, 102, 105).query();
    assert_eq!(token.output(), point(3, 500));
    assert_eq!(
        owner.validate_coordinate_input(&token, host(105)),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert!(owner.commit_coordinate_input(token, host(105)).is_err());
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn mathematically_equal_query_encodings_remain_valid() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut token = input(&owner, 101, 105);
    let original = token.query();
    token.query = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        2,
        2,
        8_000_000_000,
        0,
        1,
    )
    .unwrap();
    equal(original, token.query());
    owner.validate_coordinate_input(&token, host(105)).unwrap();
    owner.commit_coordinate_input(token, host(105)).unwrap();
    equal(owner.committed_coordinate_operation().unwrap(), original);
}

#[test]
fn observed_frontier_advances_exactly_despite_unchanged_rounded_output() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut merger = merger();
    let first = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(first.host(), host(105));
    assert_eq!(first.observed_host(), host(101));
    assert_eq!(first.epoch(), 7);
    assert_eq!(first.advance(), Some(point(3, 500)));
    owner
        .commit_coordinate_frontier(first, host(105), &mut merger)
        .unwrap();
    observe(&mut owner, 2, 104);
    let next = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(next.output(), first.output());
    assert_eq!(
        next.query().compare_interpolated_time(first.query()),
        Ordering::Greater
    );
    assert_eq!(next.advance(), Some(first.output()));
    owner
        .commit_coordinate_frontier(next, host(105), &mut merger)
        .unwrap();
    equal(
        owner.committed_coordinate_presentation().unwrap(),
        next.query(),
    );
    equal(
        owner.committed_coordinate_operation().unwrap(),
        next.query(),
    );
    assert_eq!(owner.closed_host_prefix(), Some(host(105)));
}

#[test]
fn actual_frontier_behind_prediction_closes_without_regressing_operation() {
    let mut owner = seeded(counter(4_000_000_000));
    let prediction = input(&owner, 103, 105);
    owner
        .commit_coordinate_input(prediction, host(105))
        .unwrap();
    observe(&mut owner, 2, 104);
    let mut merger = merger();
    let frontier = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(frontier.advance(), None);
    owner
        .commit_coordinate_frontier(frontier, host(105), &mut merger)
        .unwrap();
    equal(
        owner.committed_coordinate_operation().unwrap(),
        prediction.query(),
    );
    equal(
        owner.committed_coordinate_presentation().unwrap(),
        frontier.query(),
    );
    assert_eq!(owner.closed_host_prefix(), Some(host(105)));
}

#[test]
fn duplicate_frontier_and_native_duplicate_do_not_refresh_or_advance() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut merger = merger();
    let first = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    owner
        .commit_coordinate_frontier(first, host(105), &mut merger)
        .unwrap();
    let before = snapshot(&owner);
    assert!(owner
        .prepare_coordinate_frontier(host(106), &merger)
        .unwrap()
        .is_none());
    let last = owner.latest_coordinate_observation().unwrap();
    assert_eq!(
        owner.observe_coordinate(
            7,
            ClockPair {
                target: host(1000),
                ..last.pair()
            },
            last.coordinate().unwrap()
        ),
        Ok(ObservationAdmission::Unchanged)
    );
    assert_eq!(snapshot(&owner), before);
    assert!(owner
        .prepare_coordinate_frontier(host(202), &merger)
        .unwrap()
        .is_none());
}

#[test]
fn frontier_selects_due_original_observation_without_host_now_prediction() {
    let mut owner = owner(counter(4_000_000_000));
    observe(&mut owner, 1, 100);
    observe(&mut owner, 3, 200);
    owner.record_acquired_prefix(host(150)).unwrap();
    let frontier = owner
        .prepare_coordinate_frontier(host(150), &merger())
        .unwrap()
        .unwrap();
    assert_eq!(frontier.host(), host(150));
    assert_eq!(frontier.observed_host(), host(100));
    assert_eq!(
        frontier
            .query()
            .compare_native_time_with_offset(
                TargetTime::from_frames(0, 1).unwrap(),
                1,
                4_000_000_000,
                0
            )
            .unwrap(),
        Ordering::Equal
    );
}

#[test]
fn undrained_prefix_holds_and_late_pending_event_invalidates_frontier() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut merger = merger();
    let pending = event(104);
    merger.admit(pending.clone(), host(105)).unwrap();
    assert!(owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .is_none());
    assert_eq!(merger.pop_ready(host(105)).unwrap(), Some(pending.clone()));
    let frontier = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    merger.admit(event(105), host(105)).unwrap();
    let before = snapshot(&owner);
    let ready = merger.peek_ready(host(105)).unwrap().cloned();
    assert!(owner
        .commit_coordinate_frontier(frontier, host(105), &mut merger)
        .is_err());
    assert_eq!(snapshot(&owner), before);
    assert_eq!(merger.peek_ready(host(105)).unwrap().cloned(), ready);
    assert_eq!(merger.pending(), 1);
}

#[test]
fn frontier_query_tamper_freshness_and_stale_revision_leave_merger_open() {
    for mutation in 0..4 {
        let mut owner = seeded(counter(4_000_000_000));
        let mut merger = merger();
        let mut frontier = owner
            .prepare_coordinate_frontier(host(105), &merger)
            .unwrap()
            .unwrap();
        let now = match mutation {
            0 => {
                frontier.query = input(&owner, 102, 105).query();
                host(105)
            }
            1 => host(202),
            2 => {
                owner.record_acquired_prefix(host(106)).unwrap();
                host(106)
            }
            _ => host(104),
        };
        let before = snapshot(&owner);
        assert!(owner
            .commit_coordinate_frontier(frontier, now, &mut merger)
            .is_err());
        assert_eq!(snapshot(&owner), before);
        // Admission at the would-be closure demonstrates merger.commit was not called.
        merger.admit(event(105), host(105)).unwrap();
        assert_eq!(merger.pending(), 1);
    }
}

#[test]
fn revision_exhaustion_refuses_both_commits_before_either_owner_changes() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut merger = merger();
    let mut input = input(&owner, 101, 105);
    let mut frontier = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    owner.revision = u64::MAX;
    input.state = owner.state();
    frontier.state = owner.state();
    let before = snapshot(&owner);
    assert!(owner.commit_coordinate_input(input, host(105)).is_err());
    assert!(owner
        .commit_coordinate_frontier(frontier, host(105), &mut merger)
        .is_err());
    assert_eq!(snapshot(&owner), before);
    merger.admit(event(105), host(105)).unwrap();
}

#[test]
fn correlation_restart_preserves_exact_operation_and_presentation_watermarks() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut merger = merger();
    let frontier = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    owner
        .commit_coordinate_frontier(frontier, host(105), &mut merger)
        .unwrap();
    let restart = owner.prepare_correlation_restart(&merger).unwrap();
    owner.commit_correlation_restart(restart, &merger).unwrap();
    assert_eq!(owner.history_len(), 0);
    equal(
        owner.committed_coordinate_operation().unwrap(),
        frontier.query(),
    );
    equal(
        owner.committed_coordinate_presentation().unwrap(),
        frontier.query(),
    );
    assert_eq!(owner.closed_host_prefix(), Some(host(105)));
    let before = snapshot(&owner);
    let basis = owner.coordinate_basis().unwrap();
    let coordinate = NativeOutputCoordinate::Counter {
        position: 0,
        frequency: 4_000_000_000,
    };
    assert_eq!(
        owner.observe_coordinate(
            7,
            ClockPair {
                source: basis.point(coordinate).unwrap(),
                target: host(106)
            },
            coordinate
        ),
        Err(AudioAuthorityError::ObservationRegression)
    );
    assert_eq!(snapshot(&owner), before);
    observe(&mut owner, 1, 106);
    observe(&mut owner, 2, 107);
    owner.record_acquired_prefix(host(107)).unwrap();
    let next = owner
        .prepare_coordinate_frontier(host(107), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(next.advance(), Some(point(3, 500)));
    owner
        .commit_coordinate_frontier(next, host(107), &mut merger)
        .unwrap();
}

#[test]
fn original_counter_max_frequency_and_target_fraction_commit_without_narrowing() {
    for basis in [
        counter(u64::MAX),
        AudioAuthorityCoordinateBasis::TargetFrame(
            TargetFrameBasis::new(
                point(2, 0),
                TargetTime::new(0, 1, 3).unwrap(),
                2_000_000_000,
            )
            .unwrap(),
        ),
    ] {
        let mut owner = seeded(basis);
        let token = input(&owner, 101, 105);
        owner.commit_coordinate_input(token, host(105)).unwrap();
        let mut merger = merger();
        let frontier = owner
            .prepare_coordinate_frontier(host(105), &merger)
            .unwrap()
            .unwrap();
        assert_eq!(frontier.advance(), None);
        equal(frontier.query(), token.query());
        owner
            .commit_coordinate_frontier(frontier, host(105), &mut merger)
            .unwrap();
        equal(
            owner.committed_coordinate_presentation().unwrap(),
            token.query(),
        );
    }
}

#[test]
fn scalar_mapping_stays_closed_in_coordinate_mode_and_scalar_owner_refuses_new_frontier() {
    let owner = seeded(counter(4_000_000_000));
    assert_eq!(
        owner.prepare_input(host(101), host(105)).unwrap_err(),
        AudioAuthorityError::CoordinateMappingUnavailable
    );
    assert_eq!(
        owner.prepare_frontier(host(105), &merger()).unwrap_err(),
        AudioAuthorityError::CoordinateMappingUnavailable
    );
    let scalar = AudioAuthority::new(config(), owner.epoch()).unwrap();
    assert_eq!(
        scalar
            .prepare_coordinate_frontier(host(105), &merger())
            .unwrap_err(),
        AudioAuthorityError::CoordinateModeMismatch
    );
}

#[test]
fn refused_exact_regression_does_not_consume_original_merger_payload() {
    let mut owner = seeded(counter(4_000_000_000));
    let token = input(&owner, 103, 105);
    owner.commit_coordinate_input(token, host(105)).unwrap();
    observe(&mut owner, 2, 104);
    let mut merger = merger();
    let pending = event(104);
    merger.admit(pending.clone(), host(105)).unwrap();
    let before = snapshot(&owner);
    assert!(owner
        .prepare_coordinate_input(host(104), host(105))
        .is_err());
    assert_eq!(merger.peek_ready(host(105)).unwrap(), Some(&pending));
    assert_eq!(merger.pending(), 1);
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn acquired_prefix_ahead_of_now_holds_both_paths_without_owner_changes() {
    let owner = seeded(counter(4_000_000_000));
    let before = snapshot(&owner);
    assert!(owner
        .prepare_coordinate_frontier(host(104), &merger())
        .unwrap()
        .is_none());
    assert!(owner
        .prepare_coordinate_input(host(101), host(104))
        .unwrap()
        .is_none());
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn source_frame_frontier_retains_original_units_and_fractional_creation_cut() {
    let basis = OutputFrameBasis::new(point(2, 100), 3, 2).unwrap();
    let mut owner = seeded(AudioAuthorityCoordinateBasis::SourceFrame(basis));
    let token = input(&owner, 101, 105);
    assert_eq!(token.raw_output(), point(2, 1_000_000_100));
    assert_eq!(token.output(), point(3, 333_333_834));
    owner.commit_coordinate_input(token, host(105)).unwrap();
    let mut merger = merger();
    let frontier = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(
        frontier.observation().coordinate(),
        Some(NativeOutputCoordinate::StreamFrame { frame: 1 })
    );
    equal(frontier.query(), token.query());
    assert_eq!(frontier.advance(), None);
    owner
        .commit_coordinate_frontier(frontier, host(105), &mut merger)
        .unwrap();
}

#[test]
fn a_new_closed_prefix_with_equal_presentation_commits_without_advancing_operation() {
    let mut owner = seeded(counter(4_000_000_000));
    let mut merger = merger();
    let first = owner
        .prepare_coordinate_frontier(host(105), &merger)
        .unwrap()
        .unwrap();
    owner
        .commit_coordinate_frontier(first, host(105), &mut merger)
        .unwrap();
    owner.record_acquired_prefix(host(106)).unwrap();
    let next = owner
        .prepare_coordinate_frontier(host(106), &merger)
        .unwrap()
        .unwrap();
    equal(next.query(), first.query());
    assert_eq!(next.advance(), None);
    owner
        .commit_coordinate_frontier(next, host(106), &mut merger)
        .unwrap();
    assert_eq!(owner.closed_host_prefix(), Some(host(106)));
    equal(
        owner.committed_coordinate_operation().unwrap(),
        first.query(),
    );
}

#[test]
fn full_unclosed_history_refuses_original_observation_without_changing_tokens() {
    let mut owner = seeded(counter(4_000_000_000));
    for units in 2..8 {
        observe(&mut owner, units, 100 + units as i64);
    }
    owner.record_acquired_prefix(host(107)).unwrap();
    let token = input(&owner, 101, 107);
    let before = snapshot(&owner);
    let basis = owner.coordinate_basis().unwrap();
    let coordinate = NativeOutputCoordinate::Counter {
        position: 8,
        frequency: 4_000_000_000,
    };
    assert_eq!(
        owner.observe_coordinate(
            7,
            ClockPair {
                source: basis.point(coordinate).unwrap(),
                target: host(108)
            },
            coordinate
        ),
        Err(AudioAuthorityError::HistoryCapacity)
    );
    assert_eq!(snapshot(&owner), before);
    owner.validate_coordinate_input(&token, host(108)).unwrap();
}

fn restarted_input_owner(left: u64, right: u64) -> AudioAuthority {
    let mut owner = owner(counter(10_000_000_000));
    observe(&mut owner, left, 90);
    observe(&mut owner, right, 110);
    owner.record_acquired_prefix(host(100)).unwrap();
    let token = input(&owner, 100, 110);
    owner.commit_coordinate_input(token, host(110)).unwrap();
    let merger = merger();
    let restart = owner.prepare_correlation_restart(&merger).unwrap();
    owner.commit_correlation_restart(restart, &merger).unwrap();
    observe(&mut owner, 12, 200);
    observe(&mut owner, 20, 300);
    owner.record_acquired_prefix(host(250)).unwrap();
    owner
}

#[test]
fn identical_restarted_history_and_rounded_state_do_not_hide_different_exact_input_commits() {
    let first = restarted_input_owner(2, 6); // Committed 0.4 ns.
    let mut second = restarted_input_owner(6, 10); // Committed 0.8 ns.
    assert_eq!(first.revision, second.revision);
    assert_eq!(first.history, second.history);
    assert_eq!(first.committed_input_host(), second.committed_input_host());
    assert_eq!(first.committed_operation(), second.committed_operation());
    assert_eq!(first.acquired_prefix(), second.acquired_prefix());
    assert_eq!(
        first
            .committed_coordinate_operation()
            .unwrap()
            .compare_interpolated_time(second.committed_coordinate_operation().unwrap()),
        Ordering::Less
    );
    let token = input(&first, 225, 300); // 1.4 ns for both current mappers.
    assert_eq!(token.raw_output(), input(&second, 225, 300).raw_output());
    let before = snapshot(&second);
    let mut merger = merger();
    let pending = event(225);
    merger.admit(pending.clone(), host(300)).unwrap();
    assert_eq!(
        second.validate_coordinate_input(&token, host(300)),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(
        second.commit_coordinate_input(token, host(300)),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(snapshot(&second), before);
    assert_eq!(merger.peek_ready(host(250)).unwrap(), Some(&pending));
    assert_eq!(merger.pending(), 1);
}

#[test]
fn identical_restarted_history_binds_frontier_to_exact_prior_presentation_and_operation() {
    let mut first = owner(counter(10_000_000_000));
    let mut second = owner(counter(10_000_000_000));
    let mut first_merger = merger();
    let mut second_merger = merger();
    for (owner, merger, left, right) in [
        (&mut first, &mut first_merger, 2, 6),
        (&mut second, &mut second_merger, 6, 10),
    ] {
        observe(owner, left, 90);
        observe(owner, right, 110);
        owner.record_acquired_prefix(host(100)).unwrap();
        let initial = owner
            .prepare_coordinate_frontier(host(110), merger)
            .unwrap()
            .unwrap();
        owner
            .commit_coordinate_frontier(initial, host(110), merger)
            .unwrap();
        let restart = owner.prepare_correlation_restart(merger).unwrap();
        owner.commit_correlation_restart(restart, merger).unwrap();
        observe(owner, 12, 200);
        observe(owner, 20, 300);
        owner.record_acquired_prefix(host(250)).unwrap();
    }
    assert_eq!(first.revision, second.revision);
    assert_eq!(first.history, second.history);
    assert_eq!(
        first.committed_presentation(),
        second.committed_presentation()
    );
    assert_eq!(first.committed_operation(), second.committed_operation());
    assert_eq!(first.closed_host_prefix(), second.closed_host_prefix());
    let token = first
        .prepare_coordinate_frontier(host(300), &first_merger)
        .unwrap()
        .unwrap();
    let own_token = second
        .prepare_coordinate_frontier(host(300), &second_merger)
        .unwrap()
        .unwrap();
    equal(token.query(), own_token.query());
    assert_eq!(token.output(), own_token.output());
    assert_eq!(token.advance(), own_token.advance());
    let before = snapshot(&second);
    assert_eq!(
        second.commit_coordinate_frontier(token, host(300), &mut second_merger),
        Err(AudioAuthorityError::StalePreparation)
    );
    assert_eq!(snapshot(&second), before);
    // Refusal must not close the new prefix in the original merger either.
    second_merger.admit(event(250), host(300)).unwrap();
    assert_eq!(second_merger.pending(), 1);
}

#[test]
fn mathematically_equal_prior_commits_allow_tokens_after_identical_restart() {
    // Different original brackets encode the same committed 0.4 ns.
    let first = restarted_input_owner(2, 6);
    let mut second = restarted_input_owner(0, 8);
    equal(
        first.committed_coordinate_operation().unwrap(),
        second.committed_coordinate_operation().unwrap(),
    );
    assert_eq!(first.state(), second.state());
    let token = input(&first, 225, 300);
    second.validate_coordinate_input(&token, host(300)).unwrap();
    second.commit_coordinate_input(token, host(300)).unwrap();
    equal(
        second.committed_coordinate_operation().unwrap(),
        token.query(),
    );
}
