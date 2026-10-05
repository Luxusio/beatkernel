//! Deferred incomplete-frame policy fixtures; no timer, transport or clock.
use super::*;
use crate::room_setup_wait::RoomDeadlineError;

fn state() -> RoomFrameWaitState {
    RoomFrameWaitState::new(1_000_000).unwrap()
}
fn sealed(state: &mut RoomFrameWaitState) {
    for (now, pending) in [(0, false), (i64::MAX, true)] {
        assert_eq!(
            state.observe(now, pending),
            Err(RoomFrameWaitError::Finished)
        );
    }
}

#[test]
fn timeout_bounds_are_inclusive_and_invalid_configuration_cannot_create_state() {
    for timeout in [0, 999_999, 120_000_000_001, u64::MAX] {
        assert!(matches!(
            RoomFrameWaitState::new(timeout),
            Err(RoomFrameWaitError::Deadline(
                RoomDeadlineError::InvalidTimeout
            ))
        ));
    }
    for timeout in [1_000_000, 120_000_000_000] {
        let mut state = RoomFrameWaitState::new(timeout).unwrap();
        assert_eq!(state.observe(0, true), Ok(RoomFrameWaitStep::Wait(timeout)));
    }
}

#[test]
fn idle_has_no_deadline_even_after_a_week_and_observation_stays_monotonic() {
    let mut state = state();
    for now in [0, 604_800_000_000_000, 9_007_199_254_740_993] {
        assert_eq!(state.observe(now, false), Ok(RoomFrameWaitStep::Idle));
    }
    assert_eq!(
        state.observe(9_007_199_254_740_993, true),
        Ok(RoomFrameWaitStep::Wait(1_000_000))
    );
    assert_eq!(
        state.observe(9_007_199_254_740_992, true),
        Err(RoomFrameWaitError::ClockRegressed)
    );
    sealed(&mut state);
}

#[test]
fn every_partial_fragment_retains_the_first_fixed_deadline() {
    let mut state = state();
    assert_eq!(
        state.observe(100, true),
        Ok(RoomFrameWaitStep::Wait(1_000_000))
    );
    for elapsed in [1, 17, 500_000, 999_999] {
        assert_eq!(
            state.observe(100 + elapsed, true),
            Ok(RoomFrameWaitStep::Wait((1_000_000 - elapsed) as u64))
        );
    }
    assert_eq!(
        state.observe(1_000_100, true),
        Err(RoomFrameWaitError::Deadline(RoomDeadlineError::Expired))
    );
    sealed(&mut state);
}

#[test]
fn inclusive_expiry_precedes_complete_clear_and_cannot_be_retried() {
    for pending in [false, true] {
        let mut state = state();
        assert_eq!(
            state.observe(20, true),
            Ok(RoomFrameWaitStep::Wait(1_000_000))
        );
        assert_eq!(
            state.observe(1_000_020, pending),
            Err(RoomFrameWaitError::Deadline(RoomDeadlineError::Expired))
        );
        sealed(&mut state);
    }
}

#[test]
fn only_timely_complete_frame_allows_a_later_independent_frame_bound() {
    let mut state = state();
    assert_eq!(
        state.observe(10, true),
        Ok(RoomFrameWaitStep::Wait(1_000_000))
    );
    assert_eq!(state.observe(1_000_009, false), Ok(RoomFrameWaitStep::Idle));
    assert_eq!(
        state.observe(604_800_000_000_000, false),
        Ok(RoomFrameWaitStep::Idle)
    );
    assert_eq!(
        state.observe(604_800_000_000_001, true),
        Ok(RoomFrameWaitStep::Wait(1_000_000))
    );
    assert_eq!(
        state.observe(604_800_000_000_002, false),
        Ok(RoomFrameWaitStep::Idle)
    );
}

#[test]
fn negative_and_regressing_clock_errors_seal_both_idle_and_partial_states() {
    for pending in [false, true] {
        for now in [-1, 9] {
            let mut state = state();
            assert!(state.observe(10, pending).is_ok());
            let expected = if now < 0 {
                RoomFrameWaitError::Deadline(RoomDeadlineError::InvalidClock)
            } else {
                RoomFrameWaitError::ClockRegressed
            };
            assert_eq!(state.observe(now, false), Err(expected));
            sealed(&mut state);
        }
    }
}

#[test]
fn signed_extreme_deadline_preserves_exact_remaining_and_overflow_is_sealed() {
    let mut exact = state();
    assert_eq!(
        exact.observe(i64::MAX - 1_000_000, true),
        Ok(RoomFrameWaitStep::Wait(1_000_000))
    );
    assert_eq!(
        exact.observe(i64::MAX - 1, true),
        Ok(RoomFrameWaitStep::Wait(1))
    );
    assert_eq!(
        exact.observe(i64::MAX, false),
        Err(RoomFrameWaitError::Deadline(RoomDeadlineError::Expired))
    );
    sealed(&mut exact);
    let mut overflow = state();
    assert_eq!(
        overflow.observe(i64::MAX - 999_999, true),
        Err(RoomFrameWaitError::Deadline(RoomDeadlineError::Overflow))
    );
    sealed(&mut overflow);
}
