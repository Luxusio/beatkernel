//! Deferred fixed-deadline and staged setup policy fixtures; integer data only.
use super::*;

fn state(now: i64, timeout: u64) -> RoomSetupWaitState {
    match RoomSetupWaitState::new(now, timeout) {
        Ok(value) => value,
        Err(_) => panic!("valid setup refused"),
    }
}
fn step(
    state: &mut RoomSetupWaitState,
    now: i64,
    flags: (bool, bool, bool),
    expected: RoomSetupStep,
) {
    match state.step(now, flags.0, flags.1, flags.2) {
        Ok(value) => assert_eq!(value, expected),
        Err(_) => panic!("valid setup transition refused"),
    }
}
fn sealed(state: &mut RoomSetupWaitState) {
    assert!(matches!(
        state.step(i64::MAX, true, true, true),
        Err(RoomSetupError::Finished)
    ));
    assert!(matches!(
        state.step(0, false, false, false),
        Err(RoomSetupError::Finished)
    ));
}

#[test]
fn deadline_construction_refuses_bad_bounds_negative_time_and_signed_overflow() {
    assert!(matches!(
        RoomDeadline::new(-1, 1_000_000),
        Err(RoomDeadlineError::InvalidClock)
    ));
    for timeout in [0, 999_999, 120_000_000_001, u64::MAX] {
        assert!(matches!(
            RoomDeadline::new(0, timeout),
            Err(RoomDeadlineError::InvalidTimeout)
        ));
    }
    assert!(matches!(
        RoomDeadline::new(i64::MAX - 999_999, 1_000_000),
        Err(RoomDeadlineError::Overflow)
    ));
    for timeout in [1_000_000, 120_000_000_000] {
        let deadline = RoomDeadline::new(0, timeout).unwrap();
        assert_eq!(deadline.remaining_ns(0).unwrap(), timeout);
    }
    assert!(RoomSetupWaitState::new(-1, 1_000_000).is_err());
    assert!(RoomSetupWaitState::new(0, 999_999).is_err());
}

#[test]
fn deadline_integer_precision_and_inclusive_expiry_do_not_renew() {
    for now in [
        0,
        604_800_000_000_000,
        9_007_199_254_740_993,
        i64::MAX - 1_000_000,
    ] {
        let deadline = RoomDeadline::new(now, 1_000_000).unwrap();
        assert_eq!(deadline.remaining_ns(now).unwrap(), 1_000_000);
        assert_eq!(deadline.remaining_ns(now + 999_999).unwrap(), 1);
        assert!(matches!(
            deadline.remaining_ns(now + 1_000_000),
            Err(RoomDeadlineError::Expired)
        ));
        assert!(matches!(
            deadline.remaining_ns(-1),
            Err(RoomDeadlineError::InvalidClock)
        ));
        assert_eq!(deadline.remaining_ns(now + 17).unwrap(), 999_983);
    }
}

#[test]
fn admission_lobby_prepared_and_complete_use_two_distinct_fixed_scopes() {
    let mut state = state(10, 1_000_000);
    step(
        &mut state,
        10,
        (false, false, false),
        RoomSetupStep::Wait(1_000_000),
    );
    step(
        &mut state,
        900_010,
        (false, false, false),
        RoomSetupStep::Wait(100_000),
    );
    step(
        &mut state,
        900_011,
        (true, false, false),
        RoomSetupStep::Idle,
    );
    // Lobby elapsed time is not charged to the upcoming Prepared scope.
    step(
        &mut state,
        604_800_000_000_000,
        (true, false, false),
        RoomSetupStep::Idle,
    );
    let prepared = 604_800_000_000_001;
    step(
        &mut state,
        prepared,
        (true, true, false),
        RoomSetupStep::Wait(1_000_000),
    );
    step(
        &mut state,
        prepared + 999_999,
        (true, true, false),
        RoomSetupStep::Wait(1),
    );
    step(
        &mut state,
        prepared + 999_999,
        (true, true, true),
        RoomSetupStep::Complete,
    );
    // Sticky completion does not reconsider time, malformed flags or deadlines.
    step(
        &mut state,
        -1,
        (false, false, false),
        RoomSetupStep::Complete,
    );
    step(
        &mut state,
        i64::MAX,
        (true, true, true),
        RoomSetupStep::Complete,
    );
}

#[test]
fn active_deadline_precedes_late_admission_preparation_or_commit_evidence() {
    for flags in [
        (true, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        let mut state = state(0, 1_000_000);
        assert!(matches!(
            state.step(1_000_000, flags.0, flags.1, flags.2),
            Err(RoomSetupError::Deadline(
                RoomSetupPhase::Admission,
                RoomDeadlineError::Expired
            ))
        ));
        sealed(&mut state);
    }
    let mut state = state(0, 1_000_000);
    step(&mut state, 1, (true, false, false), RoomSetupStep::Idle);
    step(
        &mut state,
        20,
        (true, true, false),
        RoomSetupStep::Wait(1_000_000),
    );
    assert!(matches!(
        state.step(1_000_020, true, true, true),
        Err(RoomSetupError::Deadline(
            RoomSetupPhase::Prepared,
            RoomDeadlineError::Expired
        ))
    ));
    sealed(&mut state);
}

#[test]
fn initial_flag_combinations_and_regressing_phase_flags_refuse_atomically() {
    for mask in 0..8 {
        let mut state = state(0, 1_000_000);
        let admitted = mask & 1 != 0;
        let prepared = mask & 2 != 0;
        let committed = mask & 4 != 0;
        let result = state.step(1, admitted, prepared, committed);
        match mask {
            0 => assert!(matches!(result, Ok(RoomSetupStep::Wait(999_999)))),
            1 => assert!(matches!(result, Ok(RoomSetupStep::Idle))),
            3 => assert!(matches!(result, Ok(RoomSetupStep::Wait(1_000_000)))),
            7 => assert!(matches!(result, Ok(RoomSetupStep::Complete))),
            _ => {
                assert!(matches!(result, Err(RoomSetupError::InvalidObservation)));
                sealed(&mut state);
            }
        }
    }
    for flags in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
    ] {
        let mut state = state(0, 1_000_000);
        step(
            &mut state,
            1,
            (true, true, false),
            RoomSetupStep::Wait(1_000_000),
        );
        assert!(matches!(
            state.step(2, flags.0, flags.1, flags.2),
            Err(RoomSetupError::InvalidObservation)
        ));
        sealed(&mut state);
    }
}

#[test]
fn negative_and_regressing_observations_seal_and_never_rearm_a_pending_scope() {
    for now in [-1, 19] {
        let mut state = state(10, 1_000_000);
        step(
            &mut state,
            20,
            (false, false, false),
            RoomSetupStep::Wait(999_990),
        );
        let result = state.step(now, false, false, false);
        if now < 0 {
            assert!(matches!(
                result,
                Err(RoomSetupError::Deadline(
                    RoomSetupPhase::Admission,
                    RoomDeadlineError::InvalidClock
                ))
            ));
        } else {
            assert!(matches!(result, Err(RoomSetupError::ClockRegressed)));
        }
        sealed(&mut state);
    }
    let mut state = state(0, 1_000_000);
    step(&mut state, 1, (true, false, false), RoomSetupStep::Idle);
    assert!(matches!(
        state.step(0, true, false, false),
        Err(RoomSetupError::ClockRegressed)
    ));
    sealed(&mut state);
}

#[test]
fn late_prepared_overflow_seals_without_changing_admitted_lobby_history() {
    let mut state = state(0, 1_000_000);
    step(&mut state, 1, (true, false, false), RoomSetupStep::Idle);
    assert!(matches!(
        state.step(i64::MAX - 999_999, true, true, false),
        Err(RoomSetupError::Deadline(
            RoomSetupPhase::Prepared,
            RoomDeadlineError::Overflow
        ))
    ));
    sealed(&mut state);
}
