//! Native API typing/decoding fixtures; these do not prove IOHID hardware cadence.
use super::*;

#[test]
fn actual_runloop_reasons_remain_distinct_from_callback_queue_emptiness() {
    for (code, reason) in [
        (1, HidPollCompletion::Finished),
        (2, HidPollCompletion::Stopped),
        (3, HidPollCompletion::TimedOut),
        (4, HidPollCompletion::HandledSource),
    ] {
        assert_eq!(poll_completion(code), Ok(reason));
    }
    assert_ne!(
        HidPollCompletion::HandledSource,
        HidPollCompletion::TimedOut
    );
    assert_ne!(HidPollCompletion::Finished, HidPollCompletion::TimedOut);
    assert_ne!(HidPollCompletion::Stopped, HidPollCompletion::TimedOut);
}

#[test]
fn unknown_runloop_result_is_explicit_native_failure() {
    for code in [i32::MIN, -1, 0, 5, i32::MAX] {
        assert_eq!(poll_completion(code), Err(HidError::Native(code)));
    }
}

#[test]
fn existing_poll_signature_is_preserved_alongside_typed_completion() {
    let _: fn(&mut HidInput, Duration) -> Result<(), HidError> = HidInput::poll;
    let _: fn(&mut HidInput, Duration) -> Result<HidPollCompletion, HidError> =
        HidInput::poll_completion;
}

#[test]
fn checked_queue_success_requires_owned_value_and_other_errors_never_become_empty() {
    assert_eq!(decode_queue_result(0, true), Ok(true));
    assert!(decode_queue_result(0, false).is_err());
    for status in [-1, i32::MIN, 1, i32::MAX] {
        for value_present in [false, true] {
            assert_eq!(
                decode_queue_result(status, value_present),
                Err(HidError::Native(status))
            );
        }
    }
}

#[test]
fn queued_acquisition_signatures_preserve_checked_completion_and_existing_callback_api() {
    let _: fn(MachClock, DeviceId, usize) -> Result<HidInput, HidError> = HidInput::open_queued;
    let _: fn(&mut HidInput, &[DeviceId]) -> Result<(), QueuedInputError> =
        HidInput::select_queued_devices;
    let _: fn(&mut HidInput, usize) -> Result<Option<beatkernel::time::ClockPoint>, HidError> =
        HidInput::poll_queued;
    let _: fn(MachClock, DeviceId, usize) -> Result<HidInput, HidError> = HidInput::open;
}

#[test]
fn queued_initialization_error_keeps_primary_and_cleanup_as_distinct_causes() {
    let primary = HidError::PermissionDenied;
    let cleanup = HidError::Native(-102);
    let error = QueuedInputError {
        primary,
        cleanup: Some(cleanup),
    };
    assert_eq!(error.primary, primary);
    assert_eq!(error.cleanup, Some(cleanup));
    assert_eq!(
        std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<HidError>(),
        Some(&primary)
    );
    let message = error.to_string();
    assert!(message.starts_with(&primary.to_string()));
    assert!(message.contains(&cleanup.to_string()));
    let without_cleanup = QueuedInputError::from(primary);
    assert_eq!(without_cleanup.primary, primary);
    assert_eq!(without_cleanup.cleanup, None);
    assert_eq!(without_cleanup.to_string(), primary.to_string());
}

#[test]
fn checked_native_underrun_is_empty_only_without_a_returned_value() {
    assert_eq!(decode_queue_result(QUEUE_UNDERRUN, false), Ok(false));
    assert_eq!(
        decode_queue_result(QUEUE_UNDERRUN, true),
        Err(HidError::InvalidMetadata)
    );
    assert_eq!(
        decode_queue_result(0, false),
        Err(HidError::InvalidMetadata)
    );
    for status in [0xe000_02e2u32 as i32, 0xe000_02c1u32 as i32] {
        assert_eq!(
            decode_queue_result(status, false),
            Err(HidError::PermissionDenied)
        );
    }
}

fn point(nanos: i64) -> beatkernel::time::ClockPoint {
    beatkernel::time::ClockPoint {
        domain: beatkernel::time::ClockDomainId(2),
        timestamp: beatkernel::time::Timestamp::from_nanos(nanos),
    }
}

#[test]
fn actual_sweep_rotates_fairly_and_holds_first_cut_until_every_queue_is_empty() {
    let mut sweep = QueueSweep::new(3).unwrap();
    sweep.begin(point(10));
    assert_eq!(sweep.index(), 0);
    // Queue0 supplies a value (including an ignored conversion): no empty mark.
    assert_eq!(sweep.index(), 1);
    assert_eq!(sweep.empty(1), None);
    assert_eq!(sweep.index(), 2);
    assert_eq!(sweep.empty(2), None);
    assert_eq!(sweep.before, Some(point(10)));
    // Later service quantum/receipt cannot widen the original sweep observation.
    sweep.begin(point(1_000));
    assert_eq!(sweep.before, Some(point(10)));
    assert_eq!(sweep.index(), 0);
    assert_eq!(sweep.remaining, 1);
    assert_eq!(sweep.empty(0), Some(point(10)));
    assert_eq!(sweep.before, None);
    sweep.begin(point(2_000));
    assert_eq!(sweep.remaining, 3);
    assert_eq!(sweep.drained, vec![false, false, false]);
    assert_eq!(sweep.index(), 0);
}

#[test]
fn already_empty_queue_cannot_satisfy_another_selected_queue_or_duplicate_cut() {
    let mut sweep = QueueSweep::new(2).unwrap();
    sweep.begin(point(10));
    assert_eq!(sweep.empty(0), None);
    assert_eq!(sweep.empty(0), None);
    assert_eq!(sweep.remaining, 1);
    assert_eq!(sweep.before, Some(point(10)));
    assert_eq!(sweep.empty(1), Some(point(10)));
    assert_eq!(sweep.empty(1), None);
}

#[test]
fn native_sweep_capacity_is_explicit_for_selected_roster() {
    for count in [0, 65, usize::MAX] {
        assert!(matches!(QueueSweep::new(count), Err(HidError::Capacity)));
    }
    for count in [1, 4, 64] {
        let mut sweep = QueueSweep::new(count).unwrap();
        sweep.begin(point(10));
        for index in 0..count {
            assert_eq!(sweep.index(), index);
            assert_eq!(
                sweep.empty(index),
                (index + 1 == count).then_some(point(10))
            );
        }
    }
}

struct LeaseScript {
    calls: Vec<&'static str>,
    stop_error: Option<HidError>,
    close_error: Option<HidError>,
}
impl QueueLease for LeaseScript {
    fn stop(&mut self) -> Result<(), HidError> {
        self.calls.push("stop");
        self.stop_error.map_or(Ok(()), Err)
    }
    fn close_device(&mut self) -> Result<(), HidError> {
        self.calls.push("close");
        self.close_error.map_or(Ok(()), Err)
    }
    fn release(&mut self) {
        self.calls.push("release");
    }
}
fn cleanup(started: bool, opened: bool) -> QueueCleanup {
    QueueCleanup {
        started,
        opened,
        retained: None,
        released: false,
    }
}
fn lease(stop_error: Option<HidError>, close_error: Option<HidError>) -> LeaseScript {
    LeaseScript {
        calls: Vec::new(),
        stop_error,
        close_error,
    }
}

#[test]
fn native_queue_partial_construction_releases_only_after_owned_operations_close() {
    for (started, opened, expected) in [
        (false, false, vec!["release"]),
        (false, true, vec!["close", "release"]),
        (true, true, vec!["stop", "close", "release"]),
    ] {
        let mut state = cleanup(started, opened);
        let mut operations = lease(None, None);
        assert_eq!(state.close(&mut operations), Ok(()));
        assert_eq!(operations.calls, expected);
        assert!(!state.started);
        assert!(!state.opened);
        assert!(state.released);
        assert_eq!(state.retained, None);
        assert_eq!(state.close(&mut operations), Ok(()));
        assert_eq!(
            operations.calls, expected,
            "repeat close must not release COM references twice"
        );
    }
}

#[test]
fn native_queue_cleanup_retains_leases_and_first_error_while_attempting_both_operations() {
    let first = HidError::Native(-101);
    let second = HidError::Native(-102);
    for (stop_error, close_error, expected_error) in [
        (Some(first), Some(second), first),
        (Some(first), None, first),
        (None, Some(second), second),
    ] {
        let mut state = cleanup(true, true);
        let mut operations = lease(stop_error, close_error);
        assert_eq!(state.close(&mut operations), Err(expected_error));
        assert_eq!(operations.calls, vec!["stop", "close"]);
        assert_eq!(state.retained, Some(expected_error));
        assert_eq!(state.started, stop_error.is_some());
        assert_eq!(state.opened, close_error.is_some());
        assert!(!state.released);
        assert_eq!(state.close(&mut operations), Err(expected_error));
        assert_eq!(
            operations.calls,
            vec!["stop", "close"],
            "failed shutdown retains native leases, without release or implicit retry"
        );
    }
}

#[test]
fn native_queue_partial_open_close_failure_retains_its_native_error_without_release() {
    let mut state = cleanup(false, true);
    let mut operations = lease(None, Some(HidError::PermissionDenied));
    assert_eq!(
        state.close(&mut operations),
        Err(HidError::PermissionDenied)
    );
    assert_eq!(operations.calls, vec!["close"]);
    assert_eq!(state.retained, Some(HidError::PermissionDenied));
    assert!(!state.released);
}
