//! Actual authority and merger contracts. Mathematical tokens grant no native ACK.
use super::*;
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
        max_input_ahead: Duration::ZERO,
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
    assert_eq!(
        owner
            .observe_coordinate(
                7,
                ClockPair {
                    source: basis.point(coordinate).unwrap(),
                    target: host(at)
                },
                coordinate
            )
            .unwrap(),
        ObservationAdmission::Retained
    );
}
fn seeded() -> AudioAuthority {
    let mut owner = owner(counter(4_000_000_000));
    observe(&mut owner, 0, 100);
    observe(&mut owner, 6, 110);
    owner.record_acquired_prefix(host(110)).unwrap();
    owner
}
fn seeded_with_prediction() -> AudioAuthority {
    let basis = counter(4_000_000_000);
    let mut cfg = config();
    cfg.input_extrapolation = ExtrapolationPolicy::Bounded {
        before: Duration::from_nanos(2),
        after: Duration::from_nanos(2),
    };
    cfg.max_input_ahead = Duration::from_nanos(1);
    let mut owner = AudioAuthority::new_with_coordinates(
        cfg,
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap();
    observe(&mut owner, 0, 100);
    observe(&mut owner, 6, 110);
    // HOST 112 has genuinely been acquired, while the latest native row is
    // HOST 110. Its prediction is only 0.3ns beyond that original row.
    owner.record_acquired_prefix(host(112)).unwrap();
    owner
}
fn merger() -> InputMerger {
    InputMerger::new(ClockDomainId(1), host(i64::MIN), vec![DeviceId(9)], 8).unwrap()
}
fn event(at: i64) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: EventMeta::new(DeviceId(9), host(at), 0),
        control: PhysicalControlId::keyboard(7u16),
        state: ButtonState::Down,
    })
}
fn cut() -> InterpolatedTargetTime {
    // Original 2.6 counter ticks / 4GHz = 0.65ns. HOST 104 is 0.6ns;
    // HOST 105 is 0.75ns: the literal minimum is 105, despite identical floors.
    InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        2,
        3,
        4_000_000_000,
        3,
        5,
    )
    .unwrap()
}
fn exact_equal(left: InterpolatedTargetTime, right: InterpolatedTargetTime) {
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
fn prepare(owner: &AudioAuthority, merger: &InputMerger) -> PreparedCoordinateControlCutoff {
    owner
        .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), merger)
        .unwrap()
        .unwrap()
}

#[test]
fn exact_minimum_host_distinguishes_both_sides_of_one_rounded_nanosecond() {
    let owner = seeded();
    let before = snapshot(&owner);
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&cut(), host(110))
            .unwrap(),
        Some(host(105))
    );
    assert_eq!(
        owner
            .prepare_coordinate_input(host(104), host(110))
            .unwrap()
            .unwrap()
            .query()
            .compare_interpolated_time(cut()),
        Ordering::Less
    );
    assert_eq!(
        owner
            .prepare_coordinate_input(host(105), host(110))
            .unwrap()
            .unwrap()
            .query()
            .compare_interpolated_time(cut()),
        Ordering::Greater
    );
    assert_eq!(
        owner
            .prepare_coordinate_input(host(104), host(110))
            .unwrap()
            .unwrap()
            .raw_output(),
        point(2, 0)
    );
    assert_eq!(
        owner
            .prepare_coordinate_input(host(105), host(110))
            .unwrap()
            .unwrap()
            .raw_output(),
        point(2, 0)
    );
    for wrong in [104, 106] {
        assert_eq!(
            owner
                .prepare_coordinate_control_cutoff(7, cut(), host(wrong), host(110), &merger())
                .unwrap_err(),
            AudioAuthorityError::CoordinateMismatch
        );
    }
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn inverse_uses_original_fractional_source_counter_and_target_units() {
    for (basis, base, frequency) in [
        (
            AudioAuthorityCoordinateBasis::SourceFrame(
                OutputFrameBasis::new(point(2, 100), 3, 2).unwrap(),
            ),
            TargetTime::from_frames(2, 3).unwrap(),
            3,
        ),
        (
            counter(u64::MAX),
            TargetTime::from_frames(0, 1).unwrap(),
            u64::MAX,
        ),
        (
            AudioAuthorityCoordinateBasis::TargetFrame(
                TargetFrameBasis::new(
                    point(2, 100),
                    TargetTime::new(0, 1, 3).unwrap(),
                    2_000_000_000,
                )
                .unwrap(),
            ),
            TargetTime::new(0, 1, 3).unwrap(),
            2_000_000_000,
        ),
    ] {
        let mut owner = owner(basis);
        observe(&mut owner, 0, 100);
        observe(&mut owner, 6, 110);
        owner.record_acquired_prefix(host(110)).unwrap();
        let query = InterpolatedTargetTime::new(base, 2, 3, frequency, 3, 5).unwrap();
        // All descriptors have the same original 0 -> 6 / 10 HOST slope.
        assert_eq!(
            owner
                .presented_coordinate_boundary_host(&query, host(110))
                .unwrap(),
            Some(host(105))
        );
        let token = owner
            .prepare_coordinate_control_cutoff(7, query, host(105), host(110), &merger())
            .unwrap()
            .unwrap();
        exact_equal(token.query(), query);
        owner
            .commit_coordinate_control_cutoff(token, host(110), &merger())
            .unwrap();
        exact_equal(owner.committed_coordinate_operation().unwrap(), query);
    }
}

#[test]
fn minimum_host_literal_rational_oracle_covers_every_fraction_in_small_bracket() {
    let owner = seeded();
    for numerator in 0..=60u64 {
        let query = InterpolatedTargetTime::new(
            TargetTime::from_frames(0, 1).unwrap(),
            0,
            6,
            4_000_000_000,
            numerator as i128,
            60,
        )
        .unwrap();
        // Q=numerator/10 ticks; original HOST offset is numerator/6.
        let expected = 100 + ((numerator + 5) / 6) as i64;
        assert_eq!(
            owner
                .presented_coordinate_boundary_host(&query, host(110))
                .unwrap(),
            Some(host(expected)),
            "numerator {numerator}"
        );
    }
}

#[test]
fn inverse_requires_original_right_observation_due_and_never_extrapolates() {
    let mut single = owner(counter(4_000_000_000));
    let owner = seeded();
    let before = snapshot(&owner);
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&cut(), host(109))
            .unwrap(),
        None
    );
    let beyond = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        7,
        7,
        4_000_000_000,
        0,
        1,
    )
    .unwrap();
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&beyond, host(110))
            .unwrap(),
        None
    );
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&cut(), host(211))
            .unwrap(),
        None
    );
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&cut(), point(9, 110))
            .unwrap_err(),
        AudioAuthorityError::DomainMismatch
    );
    let before_creation = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        0,
        1,
        4_000_000_000,
        -1,
        1,
    )
    .unwrap();
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&before_creation, host(110))
            .unwrap_err(),
        AudioAuthorityError::ObservationRegression
    );
    assert_eq!(snapshot(&owner), before);
    observe(&mut single, 0, 100);
    single.record_acquired_prefix(host(110)).unwrap();
    assert_eq!(
        single
            .presented_coordinate_boundary_host(&cut(), host(110))
            .unwrap(),
        None
    );
    let mut newer = seeded();
    observe(&mut newer, 7, 211);
    // A fresh later row cannot refresh the original enclosing right endpoint.
    assert_eq!(
        newer
            .presented_coordinate_boundary_host(&cut(), host(211))
            .unwrap(),
        None
    );
}

#[test]
fn pause_drains_equality_resume_preserves_original_equality_payload() {
    let owner = seeded();
    let mut merger = merger();
    let equality = event(105);
    merger.admit(equality.clone(), host(110)).unwrap();
    assert!(owner
        .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), &merger)
        .unwrap()
        .is_none());
    let token = owner
        .prepare_coordinate_resume_control_cutoff(7, cut(), host(105), host(110), &merger)
        .unwrap()
        .unwrap();
    assert!(token.is_resume());
    owner
        .validate_coordinate_control_cutoff(&token, host(111), &merger)
        .unwrap();
    assert_eq!(merger.peek_ready(host(110)).unwrap(), Some(&equality));
    assert_eq!(merger.pop_ready(host(110)).unwrap(), Some(equality));
    assert!(!prepare(&owner, &merger).is_resume());
}

#[test]
fn both_control_kinds_require_strictly_earlier_pending_inputs_drained() {
    let owner = seeded();
    let mut merger = merger();
    merger.admit(event(104), host(110)).unwrap();
    assert!(owner
        .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), &merger)
        .unwrap()
        .is_none());
    assert!(owner
        .prepare_coordinate_resume_control_cutoff(7, cut(), host(105), host(110), &merger)
        .unwrap()
        .is_none());
    assert_eq!(merger.pending(), 1);
}

#[test]
fn accepted_control_commits_operation_only_and_does_not_close_merger() {
    let mut owner = seeded();
    let mut merger = merger();
    let token = prepare(&owner, &merger);
    assert_eq!(token.epoch(), 7);
    assert_eq!(token.host(), host(105));
    assert_eq!(token.raw_output(), point(2, 0));
    assert_eq!(token.output(), point(3, 500));
    let history = owner.history.clone();
    owner
        .commit_coordinate_control_cutoff(token, host(111), &merger)
        .unwrap();
    assert_eq!(owner.committed_operation(), Some(point(3, 500)));
    exact_equal(owner.committed_coordinate_operation().unwrap(), cut());
    assert_eq!(owner.committed_input_host(), None);
    assert_eq!(owner.committed_presentation(), None);
    assert!(owner.committed_coordinate_presentation().is_none());
    assert_eq!(owner.closed_host_prefix(), None);
    assert_eq!(owner.acquired_prefix(), Some(host(110)));
    assert_eq!(owner.history, history);
    merger.admit(event(105), host(111)).unwrap();
    assert_eq!(merger.pending(), 1);
    assert_eq!(
        owner.commit_coordinate_control_cutoff(token, host(111), &merger),
        Err(AudioAuthorityError::StalePreparation)
    );
}

#[test]
fn control_prepare_requires_complete_current_acquisition_and_matching_epoch_domains() {
    let mut owner = owner(counter(4_000_000_000));
    observe(&mut owner, 0, 100);
    observe(&mut owner, 6, 110);
    let before = snapshot(&owner);
    assert!(owner
        .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), &merger())
        .unwrap()
        .is_none());
    assert_eq!(snapshot(&owner), before);
    owner.record_acquired_prefix(host(104)).unwrap();
    assert!(owner
        .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), &merger())
        .unwrap()
        .is_none());
    owner.record_acquired_prefix(host(111)).unwrap();
    assert!(owner
        .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), &merger())
        .unwrap()
        .is_none());
    assert_eq!(
        owner
            .prepare_coordinate_control_cutoff(8, cut(), host(105), host(111), &merger())
            .unwrap_err(),
        AudioAuthorityError::WrongEpoch
    );
    assert_eq!(
        owner
            .prepare_coordinate_control_cutoff(7, cut(), point(9, 105), host(111), &merger())
            .unwrap_err(),
        AudioAuthorityError::DomainMismatch
    );
    assert_eq!(
        owner
            .prepare_coordinate_control_cutoff(7, cut(), host(105), point(9, 111), &merger())
            .unwrap_err(),
        AudioAuthorityError::DomainMismatch
    );
}

#[test]
fn control_revalidation_rejects_changed_history_acquisition_and_now_atomically() {
    for mutation in 0..5 {
        let mut owner = seeded();
        let mut merger = merger();
        let token = prepare(&owner, &merger);
        let now = match mutation {
            0 => {
                observe(&mut owner, 7, 111);
                host(111)
            }
            1 => {
                owner.record_acquired_prefix(host(111)).unwrap();
                host(111)
            }
            2 => host(109),
            3 => host(211),
            _ => point(9, 110),
        };
        let before = snapshot(&owner);
        assert!(owner
            .validate_coordinate_control_cutoff(&token, now, &merger)
            .is_err());
        assert!(owner
            .commit_coordinate_control_cutoff(token, now, &merger)
            .is_err());
        assert_eq!(snapshot(&owner), before);
        merger.admit(event(105), host(111)).unwrap();
    }
}

#[test]
fn newly_pending_input_invalidates_pause_but_equal_input_remains_valid_for_resume() {
    for resume in [false, true] {
        for at in [104, 105] {
            let mut owner = seeded();
            let mut merger = merger();
            let token = if resume {
                owner
                    .prepare_coordinate_resume_control_cutoff(
                        7,
                        cut(),
                        host(105),
                        host(110),
                        &merger,
                    )
                    .unwrap()
                    .unwrap()
            } else {
                prepare(&owner, &merger)
            };
            let payload = event(at);
            merger.admit(payload.clone(), host(110)).unwrap();
            let before = snapshot(&owner);
            if resume && at == 105 {
                owner
                    .commit_coordinate_control_cutoff(token, host(110), &merger)
                    .unwrap();
            } else {
                assert!(owner
                    .commit_coordinate_control_cutoff(token, host(110), &merger)
                    .is_err());
                assert_eq!(snapshot(&owner), before);
            }
            assert_eq!(merger.peek_ready(host(110)).unwrap(), Some(&payload));
        }
    }
}

#[test]
fn exact_control_regression_is_refused_when_logical_nanosecond_is_equal() {
    let mut owner = seeded();
    let merger = merger();
    let predicted = owner
        .prepare_coordinate_input(host(105), host(110))
        .unwrap()
        .unwrap();
    assert_eq!(predicted.output(), point(3, 500));
    owner.commit_coordinate_input(predicted, host(110)).unwrap();
    let before = snapshot(&owner);
    assert_eq!(
        owner
            .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), &merger)
            .unwrap_err(),
        AudioAuthorityError::OperationRegression
    );
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn input_host_chronology_cannot_regress_at_mathematically_equal_control_cut() {
    let mut owner = seeded_with_prediction();
    let predicted = owner
        .prepare_coordinate_input(host(112), host(112))
        .unwrap()
        .unwrap();
    owner.commit_coordinate_input(predicted, host(112)).unwrap();
    observe(&mut owner, 12, 111);
    owner.record_acquired_prefix(host(112)).unwrap();
    // The new faster bracket reaches the old predicted exact Q at HOST 111.
    // A later input HOST cannot be replaced by an earlier control HOST.
    let query = owner.committed_coordinate_operation().unwrap();
    let cutoff = owner
        .presented_coordinate_boundary_host(&query, host(112))
        .unwrap()
        .unwrap();
    assert!(cutoff.timestamp < host(112).timestamp);
    let before = snapshot(&owner);
    assert!(owner
        .prepare_coordinate_control_cutoff(7, query, cutoff, host(112), &merger())
        .is_err());
    assert_eq!(snapshot(&owner), before);
}

#[test]
fn mathematical_encoding_is_independent_for_inverse_and_prepared_token() {
    let mut owner = seeded();
    let merger = merger();
    let mut token = prepare(&owner, &merger);
    let equivalent = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        5,
        6,
        8_000_000_000,
        1,
        5,
    )
    .unwrap();
    exact_equal(cut(), equivalent);
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&equivalent, host(110))
            .unwrap(),
        Some(host(105))
    );
    token.query = equivalent;
    owner
        .validate_coordinate_control_cutoff(&token, host(110), &merger)
        .unwrap();
    owner
        .commit_coordinate_control_cutoff(token, host(110), &merger)
        .unwrap();
    exact_equal(owner.committed_coordinate_operation().unwrap(), cut());
    // Another valid mathematical cut may share the same minimum integer HOST
    // and rounded output. Native adoption must separately bind its original Q.
    let distinct = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        2,
        3,
        4_000_000_000,
        4,
        5,
    )
    .unwrap();
    assert_eq!(distinct.compare_interpolated_time(cut()), Ordering::Greater);
    let distinct_token = owner
        .prepare_coordinate_control_cutoff(7, distinct, host(105), host(110), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(distinct_token.host(), token.host());
    assert_eq!(distinct_token.raw_output(), token.raw_output());
    assert_eq!(distinct_token.output(), token.output());
    owner
        .validate_coordinate_control_cutoff(&distinct_token, host(111), &merger)
        .unwrap();
    owner
        .commit_coordinate_control_cutoff(distinct_token, host(111), &merger)
        .unwrap();
    exact_equal(owner.committed_coordinate_operation().unwrap(), distinct);
}

#[test]
fn tampered_exact_cut_and_original_token_fields_are_refused_without_grant() {
    for mutation in 0..8 {
        let mut owner = seeded();
        let merger = merger();
        let mut token = prepare(&owner, &merger);
        match mutation {
            0 => token.host_cutoff = host(104),
            1 => token.epoch = 8,
            2 => token.raw_output = point(9, 0),
            3 => token.output = point(9, 500),
            4 => token.now = host(111),
            // Genuine invalid range or a query requiring another HOST. Do not
            // require a private witness for distinct valid cuts sharing a HOST.
            5 => {
                token.query = InterpolatedTargetTime::new(
                    TargetTime::from_frames(0, 1).unwrap(),
                    0,
                    1,
                    4_000_000_000,
                    -1,
                    1,
                )
                .unwrap()
            }
            6 => {
                token.query = InterpolatedTargetTime::new(
                    TargetTime::from_frames(0, 1).unwrap(),
                    7,
                    7,
                    4_000_000_000,
                    0,
                    1,
                )
                .unwrap()
            }
            _ => {
                token.query = InterpolatedTargetTime::new(
                    TargetTime::from_frames(0, 1).unwrap(),
                    2,
                    3,
                    4_000_000_000,
                    1,
                    5,
                )
                .unwrap()
            }
        }
        let before = snapshot(&owner);
        assert!(owner
            .commit_coordinate_control_cutoff(token, host(110), &merger)
            .is_err());
        assert_eq!(snapshot(&owner), before);
    }
}

#[test]
fn held_frontier_ahead_preserves_both_operation_watermarks() {
    let mut owner = seeded();
    let mut merger = merger();
    let token = prepare(&owner, &merger);
    owner
        .commit_coordinate_control_cutoff(token, host(110), &merger)
        .unwrap();
    let logical = owner.committed_operation();
    let exact = owner.committed_coordinate_operation().unwrap();
    let held = owner
        .prepare_coordinate_held_frontier(host(110), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(held.observed_host(), host(110));
    assert_eq!(held.host(), host(110));
    assert_eq!(held.epoch(), 7);
    assert_eq!(
        held.query().compare_interpolated_time(exact),
        Ordering::Greater
    );
    owner
        .commit_coordinate_held_frontier(held, host(111), &mut merger)
        .unwrap();
    assert_eq!(owner.committed_operation(), logical);
    exact_equal(owner.committed_coordinate_operation().unwrap(), exact);
    assert_eq!(owner.committed_presentation(), Some(held.output()));
    exact_equal(
        owner.committed_coordinate_presentation().unwrap(),
        held.query(),
    );
    assert_eq!(owner.closed_host_prefix(), Some(host(110)));
    assert_eq!(owner.committed_input_host(), None);
    assert!(merger.admit(event(110), host(111)).is_err());
}

#[test]
fn held_frontier_behind_prediction_closes_without_operation_regression() {
    let mut owner = seeded_with_prediction();
    let mut merger = merger();
    let input = owner
        .prepare_coordinate_input(host(112), host(112))
        .unwrap()
        .unwrap();
    owner.commit_coordinate_input(input, host(112)).unwrap();
    let held = owner
        .prepare_coordinate_held_frontier(host(112), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(held.observed_host(), host(110));
    assert_eq!(held.host(), host(112));
    assert_eq!(
        held.query().compare_interpolated_time(input.query()),
        Ordering::Less
    );
    owner
        .commit_coordinate_held_frontier(held, host(112), &mut merger)
        .unwrap();
    assert_eq!(owner.committed_operation(), Some(input.output()));
    exact_equal(
        owner.committed_coordinate_operation().unwrap(),
        input.query(),
    );
    assert_eq!(owner.committed_input_host(), Some(host(112)));
    exact_equal(
        owner.committed_coordinate_presentation().unwrap(),
        held.query(),
    );
    assert_eq!(owner.closed_host_prefix(), Some(host(112)));
}

#[test]
fn held_closure_without_operation_does_not_manufacture_an_operation() {
    let mut owner = seeded();
    let mut merger = merger();
    let held = owner
        .prepare_coordinate_held_frontier(host(110), &merger)
        .unwrap()
        .unwrap();
    owner
        .commit_coordinate_held_frontier(held, host(110), &mut merger)
        .unwrap();
    assert_eq!(owner.committed_operation(), None);
    assert!(owner.committed_coordinate_operation().is_none());
    assert!(owner.committed_coordinate_presentation().is_some());
}

#[test]
fn held_closure_selects_due_observation_instead_of_host_now_or_latest_future_row() {
    let mut owner = owner(counter(4_000_000_000));
    observe(&mut owner, 1, 100);
    observe(&mut owner, 3, 200);
    owner.record_acquired_prefix(host(150)).unwrap();
    let mut merger = merger();
    let held = owner
        .prepare_coordinate_held_frontier(host(150), &merger)
        .unwrap()
        .unwrap();
    assert_eq!(held.observed_host(), host(100));
    assert_eq!(held.host(), host(150));
    let expected = InterpolatedTargetTime::new(
        TargetTime::from_frames(0, 1).unwrap(),
        1,
        1,
        4_000_000_000,
        0,
        1,
    )
    .unwrap();
    exact_equal(held.query(), expected);
    owner
        .commit_coordinate_held_frontier(held, host(150), &mut merger)
        .unwrap();
    exact_equal(owner.committed_coordinate_presentation().unwrap(), expected);
    assert!(owner.committed_coordinate_operation().is_none());
}

#[test]
fn held_pending_input_and_stale_token_leave_authority_and_merger_open() {
    for mutation in 0..5 {
        let mut owner = seeded();
        let mut merger = merger();
        let held = owner
            .prepare_coordinate_held_frontier(host(110), &merger)
            .unwrap()
            .unwrap();
        let now = match mutation {
            0 => {
                merger.admit(event(110), host(110)).unwrap();
                host(110)
            }
            1 => {
                owner.record_acquired_prefix(host(111)).unwrap();
                host(111)
            }
            2 => {
                observe(&mut owner, 7, 111);
                host(111)
            }
            3 => host(109),
            _ => host(211),
        };
        let before = snapshot(&owner);
        let pending = merger.pending();
        assert!(owner
            .commit_coordinate_held_frontier(held, now, &mut merger)
            .is_err());
        assert_eq!(snapshot(&owner), before);
        assert_eq!(merger.pending(), pending);
        if pending == 0 {
            merger.admit(event(110), host(111)).unwrap();
        }
    }
}

#[test]
fn signed_host_span_and_max_frequency_keep_literal_integer_minimum() {
    let mut cfg = config();
    cfg.max_observation_age = Duration::from_nanos(i64::MAX);
    let basis = counter(u64::MAX);
    let mut owner = AudioAuthority::new_with_coordinates(
        cfg,
        AudioAuthorityEpoch {
            id: 7,
            stream_origin: basis.origin().unwrap(),
            logical_origin: point(3, 500),
            host_domain: ClockDomainId(1),
        },
        basis,
    )
    .unwrap();
    observe(&mut owner, 0, i64::MIN);
    observe(&mut owner, 2, i64::MAX);
    let halfway =
        InterpolatedTargetTime::new(TargetTime::from_frames(0, 1).unwrap(), 1, 1, u64::MAX, 0, 1)
            .unwrap();
    // Half of the full odd 2^64-1 HOST span lies at -1/2; ceiling is zero.
    assert_eq!(
        owner
            .presented_coordinate_boundary_host(&halfway, host(i64::MAX))
            .unwrap(),
        Some(host(0))
    );
}

#[test]
fn retired_original_lower_bound_refuses_old_control_query() {
    let mut owner = seeded();
    let mut merger = merger();
    let held = owner
        .prepare_coordinate_held_frontier(host(110), &merger)
        .unwrap()
        .unwrap();
    owner
        .commit_coordinate_held_frontier(held, host(110), &mut merger)
        .unwrap();
    for units in 7..=13 {
        observe(&mut owner, units, 104 + units as i64);
    }
    // Capacity retirement has removed the original zero observation.
    assert_eq!(owner.history_len(), 8);
    assert_eq!(
        owner.presented_coordinate_boundary_host(&cut(), host(117)),
        Err(AudioAuthorityError::HistoryExpired)
    );
}

#[test]
fn scalar_authority_refuses_coordinate_control_and_held_seams() {
    let coordinate = seeded();
    let scalar = AudioAuthority::new(config(), coordinate.epoch()).unwrap();
    assert_eq!(
        scalar
            .presented_coordinate_boundary_host(&cut(), host(110))
            .unwrap_err(),
        AudioAuthorityError::CoordinateModeMismatch
    );
    assert_eq!(
        scalar
            .prepare_coordinate_control_cutoff(7, cut(), host(105), host(110), &merger())
            .unwrap_err(),
        AudioAuthorityError::CoordinateModeMismatch
    );
    assert_eq!(
        scalar
            .prepare_coordinate_held_frontier(host(110), &merger())
            .unwrap_err(),
        AudioAuthorityError::CoordinateModeMismatch
    );
}
