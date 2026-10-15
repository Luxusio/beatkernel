use super::*;
use crate::practice_control::*;
use beatkernel::audio::{
    PracticeBoundaryKind, PracticeRateEvent, PracticeRateOutcome, PracticeRateRefusal,
    PracticeReceipt, PracticeRestartEvent,
};
use beatkernel::transport::Rate as ControlRate;

fn control_rate(numerator: i64, denominator: u64) -> ControlRate {
    ControlRate::new(numerator, denominator).unwrap()
}

fn transaction_cap() -> PracticeTransactionCapability {
    PracticeTransactionCapability {
        practice: cap(),
        rate: PracticeRateState {
            revision: 7,
            rate: ControlRate::NORMAL,
        },
    }
}

fn typed_controls() -> PracticeControls {
    let mut controls = PracticeControls::new();
    controls
        .advertise_transactions(Some(transaction_cap()))
        .unwrap();
    controls
}

// These copied core DTOs prove mailbox validation and correlation only. The
// actual PCM/phase transition is exercised by the independent audio fixtures.
fn copied_rate_event(request: PracticeTransactionRequest) -> PracticeRateEvent {
    let PracticeTransactionRequest::Rate {
        id,
        generation,
        expected,
        requested_rate,
    } = request
    else {
        panic!("rate request required")
    };
    PracticeRateEvent {
        request_id: id,
        generation,
        iteration: 0,
        request_revision: expected.revision,
        applied_revision: expected.revision + 1,
        physical_frame: 200,
        playback_frame: 150,
        active_elapsed_nanos: 3_125_000,
        previous_rate: expected.rate,
        requested_rate,
        applied_rate: requested_rate,
        outcome: if requested_rate == expected.rate {
            PracticeRateOutcome::Noop
        } else {
            PracticeRateOutcome::Applied
        },
    }
}

fn copied_rate_reply(
    request: PracticeTransactionRequest,
    event: PracticeRateEvent,
) -> PracticeTransactionReply {
    let PracticeTransactionRequest::Rate { id, generation, .. } = request else {
        panic!("rate request required")
    };
    PracticeTransactionReply::Rate {
        id,
        generation,
        result: Ok(event),
    }
}

fn copied_restart_event(request: PracticeTransactionRequest) -> PracticeRestartEvent {
    let PracticeTransactionRequest::RestartOriginal {
        id,
        generation,
        expected,
    } = request
    else {
        panic!("restart request required")
    };
    PracticeRestartEvent {
        boundary: PracticeReceipt {
            request_id: id,
            generation: generation + 1,
            iteration: 0,
            physical_frame: 200,
            playback_frame: 150,
            requested_song_time: t(10),
            applied_song_time: t(10),
            correction_nanos: 0,
            kind: PracticeBoundaryKind::Requested,
        },
        request_revision: expected.revision,
        applied_revision: expected.revision + 1,
        previous_rate: expected.rate,
        requested_rate: ControlRate::NORMAL,
        applied_rate: ControlRate::NORMAL,
        outcome: PracticeRateOutcome::Applied,
    }
}

fn copied_restart_reply(
    request: PracticeTransactionRequest,
    event: PracticeRestartEvent,
) -> PracticeTransactionReply {
    let PracticeTransactionRequest::RestartOriginal { id, generation, .. } = request else {
        panic!("restart request required")
    };
    PracticeTransactionReply::RestartOriginal {
        id,
        generation,
        result: Ok(event),
    }
}

fn assert_transaction_untouched(
    controls: &mut PracticeControls,
    capability: PracticeTransactionCapability,
    request: PracticeTransactionRequest,
) {
    assert_eq!(controls.transaction_capability(), Some(capability));
    assert_eq!(controls.capability(), Some(capability.practice));
    assert_eq!(controls.pending_transaction(), Some(request));
    assert!(controls.pending());
    assert!(controls.take_transaction_response().is_none());
    assert!(controls.take_transaction(true).is_none());
}

#[test]
fn typed_practice_advertisement_is_explicit_and_invalid_scalar_changes_are_atomic() {
    let mut controls = PracticeControls::new();
    assert!(controls.transaction_capability().is_none());
    assert!(controls.request_rate(control_rate(2, 1)).is_err());
    assert!(controls.request_original_restart().is_err());
    controls.advertise(Some(cap())).unwrap();
    assert_eq!(controls.capability(), Some(cap()));
    assert!(controls.transaction_capability().is_none());
    assert!(controls.request_rate(control_rate(2, 1)).is_err());
    assert!(controls.request_original_restart().is_err());
    controls
        .advertise_transactions(Some(transaction_cap()))
        .unwrap();
    assert_eq!(controls.transaction_capability(), Some(transaction_cap()));
    for invalid_rate in [control_rate(0, 1), control_rate(-1, 2)] {
        let mut invalid = transaction_cap();
        invalid.rate.rate = invalid_rate;
        assert!(controls.advertise_transactions(Some(invalid)).is_err());
        assert_eq!(controls.transaction_capability(), Some(transaction_cap()));
        assert!(controls.request_rate(invalid_rate).is_err());
        assert!(!controls.pending());
    }
    let mut invalid = transaction_cap();
    invalid.practice.min_target = t(2);
    invalid.practice.max_target = t(1);
    assert!(controls.advertise_transactions(Some(invalid)).is_err());
    assert_eq!(controls.transaction_capability(), Some(transaction_cap()));
    assert_eq!(controls.capability(), Some(cap()));
    controls.advertise_transactions(None).unwrap();
    assert!(controls.transaction_capability().is_none());
    assert!(controls.capability().is_none());
}

#[test]
fn typed_practice_rate_region_and_restart_share_one_slot_and_one_id_allocator() {
    let mut controls = typed_controls();
    let mut previous_id = 0;
    for operation in 0..3 {
        let id = match operation {
            0 => controls.request_rate(control_rate(2, 1)).unwrap(),
            1 => controls
                .request(PracticeAction::Scrub { target: t(20) })
                .unwrap(),
            _ => controls.request_original_restart().unwrap(),
        };
        assert!(id > previous_id);
        previous_id = id;
        let request = controls.pending_transaction().unwrap();
        assert!(controls.request_rate(control_rate(3, 2)).is_err());
        assert!(controls.request(PracticeAction::DisableLoop).is_err());
        assert!(controls.request_original_restart().is_err());
        assert_eq!(controls.take_transaction(true), Some(request));
        assert!(controls.take_transaction(true).is_none());
        let reply = match request {
            PracticeTransactionRequest::Region(request) => PracticeTransactionReply::Region {
                reply: PracticeReply {
                    id: request.id,
                    generation: request.generation,
                    result: Err("pre-cut queue admission refused".into()),
                },
                rate: None,
            },
            PracticeTransactionRequest::Rate { id, generation, .. } => {
                PracticeTransactionReply::Rate {
                    id,
                    generation,
                    result: Err("pre-cut queue admission refused".into()),
                }
            }
            PracticeTransactionRequest::RestartOriginal { id, generation, .. } => {
                PracticeTransactionReply::RestartOriginal {
                    id,
                    generation,
                    result: Err("pre-cut queue admission refused".into()),
                }
            }
        };
        controls.reply_transaction(&reply).unwrap();
        assert!(controls.pending()); // An unconsumed reply still owns the slot.
        assert_eq!(controls.pending_transaction(), Some(request));
        assert!(controls.request_rate(control_rate(3, 2)).is_err());
        assert!(controls.request(PracticeAction::DisableLoop).is_err());
        assert!(controls.request_original_restart().is_err());
        let response = controls.take_transaction_response().unwrap();
        assert_eq!(response.request, request);
        assert_eq!(response.reply, reply);
        assert!(!controls.pending());
        assert!(controls.pending_transaction().is_none());
        assert!(controls.take_transaction_response().is_none());
    }
}

#[test]
fn typed_practice_rate_waits_while_paused_without_consuming_or_duplicating_request() {
    let mut controls = typed_controls();
    let id = controls.request_rate(control_rate(3, 2)).unwrap();
    let request = controls.pending_transaction().unwrap();
    assert!(controls.rate_pending());
    for _ in 0..3 {
        assert!(controls.take_transaction(false).is_none());
        assert_eq!(controls.pending_transaction(), Some(request));
        assert!(controls.rate_pending());
    }
    assert_eq!(controls.take_transaction(true), Some(request));
    assert!(controls.take_transaction(true).is_none());
    assert!(controls.take_transaction(false).is_none());
    assert!(controls.rate_pending());
    let event = copied_rate_event(request);
    assert_eq!(event.request_id, id);
    controls
        .reply_transaction(&copied_rate_reply(request, event))
        .unwrap();
    assert!(controls.rate_pending());
    assert_eq!(
        controls.take_transaction_response().unwrap().request,
        request
    );
    assert!(!controls.rate_pending());
    controls.request(PracticeAction::DisableLoop).unwrap();
    assert!(!controls.rate_pending());
}

#[test]
fn typed_practice_legacy_probes_never_consume_any_typed_variant() {
    for operation in 0..3 {
        let mut controls = typed_controls();
        match operation {
            0 => controls.request_rate(control_rate(2, 1)).unwrap(),
            1 => controls
                .request(PracticeAction::Scrub { target: t(10) })
                .unwrap(),
            _ => controls.request_original_restart().unwrap(),
        };
        let request = controls.pending_transaction().unwrap();
        assert!(controls.pending_request().is_none());
        assert!(controls.take_request().is_none());
        assert!(controls.take_response().is_none());
        let legacy_request = PracticeRequest {
            id: match request {
                PracticeTransactionRequest::Region(request) => request.id,
                PracticeTransactionRequest::Rate { id, .. }
                | PracticeTransactionRequest::RestartOriginal { id, .. } => id,
            },
            generation: 1,
            action: PracticeAction::Scrub { target: t(10) },
        };
        assert!(controls.reply(&applied(legacy_request, 2)).is_err());
        assert_eq!(controls.take_transaction(true), Some(request));
        let reply = match request {
            PracticeTransactionRequest::Rate { .. } => {
                copied_rate_reply(request, copied_rate_event(request))
            }
            PracticeTransactionRequest::RestartOriginal { .. } => {
                copied_restart_reply(request, copied_restart_event(request))
            }
            PracticeTransactionRequest::Region(request) => PracticeTransactionReply::Region {
                reply: applied(request, 2),
                rate: Some(PracticeRateState {
                    revision: 8,
                    rate: ControlRate::NORMAL,
                }),
            },
        };
        controls.reply_transaction(&reply).unwrap();
        assert!(controls.take_response().is_none());
        assert!(controls.pending());
        assert_eq!(controls.take_transaction_response().unwrap().reply, reply);
    }
}

#[test]
fn typed_practice_legacy_region_request_and_response_remain_compatible() {
    let mut controls = PracticeControls::new();
    controls.advertise(Some(cap())).unwrap();
    controls
        .request(PracticeAction::Scrub { target: t(10) })
        .unwrap();
    let request = controls.pending_request().unwrap();
    assert!(controls.pending_transaction().is_none());
    assert!(controls.take_transaction(true).is_none());
    assert!(controls.take_transaction_response().is_none());
    assert_eq!(controls.pending_request(), Some(request));
    assert_eq!(controls.take_request(), Some(request));
    assert!(controls.take_request().is_none());
    let reply = applied(request, 2);
    controls.reply(&reply).unwrap();
    assert!(controls.take_transaction_response().is_none());
    let response = controls.take_response().unwrap();
    assert_eq!(response.request, request);
    assert_eq!(response.reply, reply);
    assert!(controls.transaction_capability().is_none());
    assert_eq!(controls.capability().unwrap().generation, 2);
}

#[test]
fn typed_practice_applied_and_noop_consume_one_revision_in_the_same_attempt() {
    for requested_rate in [control_rate(2, 1), control_rate(2, 2)] {
        let mut controls = typed_controls();
        controls.request_rate(requested_rate).unwrap();
        let request = controls.take_transaction(true).unwrap();
        let event = copied_rate_event(request);
        let reply = copied_rate_reply(request, event);
        controls.reply_transaction(&reply).unwrap();
        assert_eq!(
            controls.transaction_capability(),
            Some(PracticeTransactionCapability {
                practice: cap(),
                rate: PracticeRateState {
                    revision: 8,
                    rate: requested_rate,
                },
            })
        );
        assert!(controls.reply_transaction(&reply).is_err());
        assert!(controls.take_transaction(true).is_none());
        let response = controls.take_transaction_response().unwrap();
        assert_eq!(response.request, request);
        assert_eq!(response.reply, reply);
        assert!(controls.reply_transaction(&reply).is_err());
        assert!(controls.take_transaction_response().is_none());
        assert_eq!(controls.capability().unwrap().generation, 1);
    }
}

#[test]
fn typed_practice_reply_before_take_and_wrong_variant_do_not_publish_success() {
    let mut controls = typed_controls();
    controls.request_rate(control_rate(2, 1)).unwrap();
    let request = controls.pending_transaction().unwrap();
    let reply = copied_rate_reply(request, copied_rate_event(request));
    assert!(controls.reply_transaction(&reply).is_err());
    assert_eq!(controls.transaction_capability(), Some(transaction_cap()));
    assert_eq!(controls.take_transaction(true), Some(request));
    let PracticeTransactionRequest::Rate { id, generation, .. } = request else {
        unreachable!()
    };
    let wrong = PracticeTransactionReply::RestartOriginal {
        id,
        generation,
        result: Err("wrong variant".into()),
    };
    assert!(controls.reply_transaction(&wrong).is_err());
    assert_transaction_untouched(&mut controls, transaction_cap(), request);
    controls.reply_transaction(&reply).unwrap();
}

#[test]
fn typed_practice_invalid_copied_rate_facts_are_rejected_without_partial_commit() {
    // No PCM is manufactured here: these values exercise the DTO trust boundary.
    for fault in 0..17 {
        let mut controls = typed_controls();
        controls.request_rate(control_rate(2, 1)).unwrap();
        let request = controls.take_transaction(true).unwrap();
        let valid = copied_rate_event(request);
        let mut event = valid;
        match fault {
            0 => event.request_id += 1,
            1 => event.generation += 1,
            2 => event.request_revision += 1,
            3 => event.applied_revision = event.request_revision,
            4 => event.applied_revision += 1,
            5 => event.previous_rate = control_rate(3, 2),
            6 => event.requested_rate = control_rate(3, 2),
            7 => event.applied_rate = control_rate(3, 2),
            8 => event.physical_frame = event.playback_frame - 1,
            9 => event.active_elapsed_nanos = -1,
            10 => event.previous_rate = control_rate(0, 1),
            11 => event.requested_rate = control_rate(-1, 1),
            12 => event.applied_rate = control_rate(0, 1),
            13 => event.outcome = PracticeRateOutcome::Noop,
            14 => event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity),
            15 | 16 => {}
            _ => unreachable!(),
        }
        let mut reply = copied_rate_reply(request, event);
        if let PracticeTransactionReply::Rate { id, generation, .. } = &mut reply {
            if fault == 15 {
                *id += 1;
            } else if fault == 16 {
                *generation += 1;
            }
        }
        assert!(controls.reply_transaction(&reply).is_err(), "fault {fault}");
        assert_transaction_untouched(&mut controls, transaction_cap(), request);
        controls
            .reply_transaction(&copied_rate_reply(request, valid))
            .unwrap();
        assert_eq!(controls.transaction_capability().unwrap().rate.revision, 8);
    }
}

#[test]
fn typed_practice_same_rate_requires_noop_instead_of_applied() {
    let mut controls = typed_controls();
    controls.request_rate(control_rate(8, 8)).unwrap();
    let request = controls.take_transaction(true).unwrap();
    let valid = copied_rate_event(request);
    assert_eq!(valid.outcome, PracticeRateOutcome::Noop);
    let mut invalid = valid;
    invalid.outcome = PracticeRateOutcome::Applied;
    assert!(controls
        .reply_transaction(&copied_rate_reply(request, invalid))
        .is_err());
    assert_transaction_untouched(&mut controls, transaction_cap(), request);
    controls
        .reply_transaction(&copied_rate_reply(request, valid))
        .unwrap();
}

#[test]
fn typed_practice_pre_cut_error_and_actual_audio_refusal_remain_distinct() {
    for actual_cut in [false, true] {
        let mut controls = typed_controls();
        controls.request_rate(control_rate(2, 1)).unwrap();
        let request = controls.take_transaction(true).unwrap();
        let PracticeTransactionRequest::Rate { id, generation, .. } = request else {
            unreachable!()
        };
        let reply = if actual_cut {
            let mut event = copied_rate_event(request);
            event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity);
            event.applied_revision = 7;
            event.applied_rate = ControlRate::NORMAL;
            copied_rate_reply(request, event)
        } else {
            PracticeTransactionReply::Rate {
                id,
                generation,
                result: Err("queue refused before active PCM boundary".into()),
            }
        };
        controls.reply_transaction(&reply).unwrap();
        assert_eq!(controls.transaction_capability(), Some(transaction_cap()));
        let response = controls.take_transaction_response().unwrap();
        assert_eq!(response.request, request);
        assert_eq!(response.reply, reply);
        let PracticeTransactionReply::Rate { result, .. } = response.reply else {
            unreachable!()
        };
        if actual_cut {
            let event = result.unwrap();
            assert_eq!(event.physical_frame, 200);
            assert_eq!(event.playback_frame, 150);
            assert_eq!(
                event.outcome,
                PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity)
            );
        } else {
            assert_eq!(
                result.unwrap_err(),
                "queue refused before active PCM boundary"
            );
        }
    }
}

#[test]
fn typed_practice_actual_refusal_cannot_change_rate_or_revision() {
    for fault in 0..2 {
        let mut controls = typed_controls();
        controls.request_rate(control_rate(2, 1)).unwrap();
        let request = controls.take_transaction(true).unwrap();
        let mut event = copied_rate_event(request);
        event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity);
        event.applied_rate = ControlRate::NORMAL;
        event.applied_revision = 7;
        let valid = event;
        if fault == 0 {
            event.applied_revision += 1;
        } else {
            event.applied_rate = control_rate(2, 1);
        }
        assert!(controls
            .reply_transaction(&copied_rate_reply(request, event))
            .is_err());
        assert_transaction_untouched(&mut controls, transaction_cap(), request);
        // Current and requested generation/revision still agree. A capacity
        // refusal cannot truthfully be relabelled as superseded control state.
        for wrong_reason in [
            PracticeRateRefusal::StaleRevision,
            PracticeRateRefusal::StaleGeneration,
        ] {
            let mut mislabeled = valid;
            mislabeled.outcome = PracticeRateOutcome::Refused(wrong_reason);
            assert!(controls
                .reply_transaction(&copied_rate_reply(request, mislabeled))
                .is_err());
            assert_transaction_untouched(&mut controls, transaction_cap(), request);
        }
        controls
            .reply_transaction(&copied_rate_reply(request, valid))
            .unwrap();
        assert_eq!(controls.transaction_capability(), Some(transaction_cap()));
    }
}

#[test]
fn typed_practice_stale_generation_facts_preserve_the_newer_advertisement() {
    let mut controls = typed_controls();
    controls.request_rate(control_rate(2, 1)).unwrap();
    let request = controls.take_transaction(true).unwrap();
    let current = PracticeTransactionCapability {
        practice: PracticeCapability {
            generation: 2,
            ..cap()
        },
        rate: PracticeRateState {
            revision: 8,
            rate: control_rate(1, 2),
        },
    };
    controls.advertise_transactions(Some(current)).unwrap();
    let mut event = copied_rate_event(request);
    event.generation = 2;
    event.previous_rate = current.rate.rate;
    event.applied_rate = current.rate.rate;
    event.applied_revision = current.rate.revision;
    event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::StaleGeneration);
    let mut wrong_actual_attempt = event;
    wrong_actual_attempt.generation = 3;
    assert!(controls
        .reply_transaction(&copied_rate_reply(request, wrong_actual_attempt))
        .is_err());
    assert_transaction_untouched(&mut controls, current, request);
    let reply = copied_rate_reply(request, event);
    controls.reply_transaction(&reply).unwrap();
    assert_eq!(controls.transaction_capability(), Some(current));
    let response = controls.take_transaction_response().unwrap();
    assert_eq!(response.request, request);
    assert_eq!(response.reply, reply);
    let PracticeTransactionReply::Rate {
        generation, result, ..
    } = response.reply
    else {
        unreachable!()
    };
    assert_eq!(generation, 1); // Original request, not the later actual attempt.
    assert_eq!(result.unwrap().generation, 2);
    assert_eq!(controls.transaction_capability(), Some(current));
}

#[test]
fn typed_practice_stale_revision_refusal_preserves_the_actual_current_state() {
    let mut controls = typed_controls();
    controls.request_rate(control_rate(2, 1)).unwrap();
    let request = controls.take_transaction(true).unwrap();
    let current = PracticeTransactionCapability {
        practice: cap(),
        rate: PracticeRateState {
            revision: 8,
            rate: control_rate(1, 2),
        },
    };
    controls.advertise_transactions(Some(current)).unwrap();
    let mut event = copied_rate_event(request);
    event.previous_rate = current.rate.rate;
    event.applied_rate = current.rate.rate;
    event.applied_revision = current.rate.revision;
    event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::StaleRevision);
    let mut stale_fabrication = event;
    stale_fabrication.applied_revision = 7;
    stale_fabrication.previous_rate = ControlRate::NORMAL;
    stale_fabrication.applied_rate = ControlRate::NORMAL;
    assert!(controls
        .reply_transaction(&copied_rate_reply(request, stale_fabrication))
        .is_err());
    assert_transaction_untouched(&mut controls, current, request);
    let reply = copied_rate_reply(request, event);
    controls.reply_transaction(&reply).unwrap();
    assert_eq!(controls.transaction_capability(), Some(current));
    assert_eq!(controls.take_transaction_response().unwrap().reply, reply);
}

#[test]
fn typed_practice_revoke_cancels_once_without_admitting_a_late_actual_reply() {
    for taken in [false, true] {
        let mut controls = typed_controls();
        controls.request_rate(control_rate(2, 1)).unwrap();
        let request = controls.pending_transaction().unwrap();
        if taken {
            assert_eq!(controls.take_transaction(true), Some(request));
        }
        controls.revoke("cancelled by owner");
        controls.revoke("later cleanup");
        assert!(controls.capability().is_none());
        assert!(controls.transaction_capability().is_none());
        assert!(controls.pending());
        assert!(controls
            .reply_transaction(&copied_rate_reply(request, copied_rate_event(request)))
            .is_err());
        let response = controls.take_transaction_response().unwrap();
        assert_eq!(response.request, request);
        let PracticeTransactionReply::Rate { result, .. } = response.reply else {
            unreachable!()
        };
        assert_eq!(result.unwrap_err(), "cancelled by owner");
        assert!(!controls.pending());
        assert!(!controls.rate_pending());
        assert!(controls.take_transaction_response().is_none());
        assert!(controls.request_rate(control_rate(2, 1)).is_err());
    }
}

#[test]
fn typed_practice_revoke_preserves_an_already_published_actual_result() {
    let mut controls = typed_controls();
    controls.request_rate(control_rate(2, 1)).unwrap();
    let request = controls.take_transaction(true).unwrap();
    let reply = copied_rate_reply(request, copied_rate_event(request));
    controls.reply_transaction(&reply).unwrap();
    controls.revoke("owner exited after the actual cut");
    assert!(controls.pending());
    assert!(controls.transaction_capability().is_none());
    let response = controls.take_transaction_response().unwrap();
    assert_eq!(response.request, request);
    assert_eq!(response.reply, reply);
}

#[test]
fn typed_practice_region_results_preserve_rate_and_advance_the_explicit_revision() {
    for action in [
        PracticeAction::Scrub { target: t(10) },
        PracticeAction::Loop {
            start: t(10),
            end: t(20),
        },
        PracticeAction::DisableLoop,
    ] {
        let mut controls = typed_controls();
        controls.request(action).unwrap();
        let request = controls.take_transaction(true).unwrap();
        let PracticeTransactionRequest::Region(region) = request else {
            unreachable!()
        };
        let generation = if action == PracticeAction::DisableLoop {
            1
        } else {
            2
        };
        let reply = PracticeTransactionReply::Region {
            reply: applied(region, generation),
            rate: Some(PracticeRateState {
                revision: 8,
                rate: ControlRate::NORMAL,
            }),
        };
        controls.reply_transaction(&reply).unwrap();
        let current = controls.transaction_capability().unwrap();
        assert_eq!(current.practice.generation, generation);
        assert_eq!(current.rate.revision, 8);
        assert_eq!(current.rate.rate, ControlRate::NORMAL);
        assert_eq!(controls.take_transaction_response().unwrap().reply, reply);
    }
}

#[test]
fn typed_practice_region_cannot_omit_or_change_its_frozen_rate_state() {
    for invalid_rate in [
        None,
        Some(PracticeRateState {
            revision: 7,
            rate: ControlRate::NORMAL,
        }),
        Some(PracticeRateState {
            revision: 9,
            rate: ControlRate::NORMAL,
        }),
        Some(PracticeRateState {
            revision: 8,
            rate: control_rate(2, 1),
        }),
    ] {
        let mut controls = typed_controls();
        controls
            .request(PracticeAction::Scrub { target: t(10) })
            .unwrap();
        let request = controls.take_transaction(true).unwrap();
        let PracticeTransactionRequest::Region(region) = request else {
            unreachable!()
        };
        let invalid = PracticeTransactionReply::Region {
            reply: applied(region, 2),
            rate: invalid_rate,
        };
        assert!(controls.reply_transaction(&invalid).is_err());
        assert_transaction_untouched(&mut controls, transaction_cap(), request);
    }
}

#[test]
fn typed_practice_restart_is_one_actual_generation_and_revision_transition() {
    let mut controls = PracticeControls::new();
    let mut capability = transaction_cap();
    capability.rate.rate = control_rate(2, 1);
    controls.advertise_transactions(Some(capability)).unwrap();
    controls.request_original_restart().unwrap();
    assert!(!controls.rate_pending());
    let request = controls.take_transaction(true).unwrap();
    let event = copied_restart_event(request);
    let reply = copied_restart_reply(request, event);
    controls.reply_transaction(&reply).unwrap();
    assert!(controls.reply_transaction(&reply).is_err());
    let current = controls.transaction_capability().unwrap();
    assert_eq!(current.practice.generation, 2);
    assert_eq!(current.rate.revision, 8);
    assert_eq!(current.rate.rate, ControlRate::NORMAL);
    let response = controls.take_transaction_response().unwrap();
    assert_eq!(response.request, request);
    assert_eq!(response.reply, reply);
    assert!(controls.take_transaction_response().is_none());
}

#[test]
fn typed_practice_invalid_restart_facts_do_not_move_the_old_anchor() {
    for fault in 0..15 {
        let mut controls = typed_controls();
        controls.request_original_restart().unwrap();
        let request = controls.take_transaction(true).unwrap();
        let valid = copied_restart_event(request);
        let mut event = valid;
        match fault {
            0 => event.boundary.request_id += 1,
            1 => event.boundary.generation -= 1,
            2 => event.boundary.generation += 1,
            3 => event.request_revision += 1,
            4 => event.applied_revision -= 1,
            5 => event.previous_rate = control_rate(2, 1),
            6 => event.requested_rate = control_rate(0, 1),
            7 => event.applied_rate = control_rate(2, 1),
            8 => event.boundary.physical_frame = event.boundary.playback_frame - 1,
            9 => event.boundary.kind = PracticeBoundaryKind::Looped,
            10 => event.outcome = PracticeRateOutcome::Noop,
            11 => event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::StaleRevision),
            12 => event.boundary.iteration = 1,
            13 => event.boundary.applied_song_time = t(11),
            14 => event.boundary.correction_nanos = 1,
            _ => unreachable!(),
        }
        assert!(
            controls
                .reply_transaction(&copied_restart_reply(request, event))
                .is_err(),
            "fault {fault}"
        );
        assert_transaction_untouched(&mut controls, transaction_cap(), request);
        controls
            .reply_transaction(&copied_restart_reply(request, valid))
            .unwrap();
    }
}

#[test]
fn typed_practice_restart_refusal_marker_keeps_old_generation_and_revision() {
    let mut controls = PracticeControls::new();
    let mut capability = transaction_cap();
    capability.rate.rate = control_rate(2, 1);
    controls.advertise_transactions(Some(capability)).unwrap();
    controls.request_original_restart().unwrap();
    let request = controls.take_transaction(true).unwrap();
    let mut event = copied_restart_event(request);
    event.boundary.generation = 1;
    event.boundary.kind = PracticeBoundaryKind::ControlRejectedRevision;
    event.applied_revision = 7;
    event.applied_rate = capability.rate.rate;
    event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity);
    let mut wrong_marker = event;
    wrong_marker.boundary.kind = PracticeBoundaryKind::Requested;
    assert!(controls
        .reply_transaction(&copied_restart_reply(request, wrong_marker))
        .is_err());
    assert_transaction_untouched(&mut controls, capability, request);
    for wrong_reason in [
        PracticeRateRefusal::StaleRevision,
        PracticeRateRefusal::StaleGeneration,
    ] {
        let mut mislabeled = event;
        mislabeled.outcome = PracticeRateOutcome::Refused(wrong_reason);
        assert!(controls
            .reply_transaction(&copied_restart_reply(request, mislabeled))
            .is_err());
        assert_transaction_untouched(&mut controls, capability, request);
    }
    let reply = copied_restart_reply(request, event);
    controls.reply_transaction(&reply).unwrap();
    assert_eq!(controls.transaction_capability(), Some(capability));
    assert_eq!(controls.take_transaction_response().unwrap().reply, reply);
}
fn t(ns: i64) -> Timestamp {
    Timestamp::from_nanos(ns)
}
fn cap() -> PracticeCapability {
    PracticeCapability {
        generation: 1,
        min_target: t(0),
        max_target: t(604_800_000_000_000),
    }
}
fn applied(request: PracticeRequest, generation: u64) -> PracticeReply {
    let target = match request.action {
        PracticeAction::Scrub { target } => target,
        PracticeAction::Loop { start, .. } => start,
        PracticeAction::DisableLoop => t(10),
    };
    PracticeReply {
        id: request.id,
        generation: request.generation,
        result: Ok(PracticeApplied {
            generation,
            physical_frame: 200,
            playback_frame: 150,
            requested_target: target,
            applied_target: target,
        }),
    }
}
#[test]
fn practice_control_response_retains_originating_action_across_actual_lock_contention() {
    for action in [
        PracticeAction::Scrub { target: t(10) },
        PracticeAction::Loop {
            start: t(10),
            end: t(20),
        },
        PracticeAction::DisableLoop,
    ] {
        let (publisher, viewer) = channel();
        publisher.advertise_practice(Some(cap())).unwrap();
        let id = viewer.request_practice(action).unwrap();
        let request = publisher.take_practice_request().unwrap().unwrap();
        publisher
            .commit_practice_reply(&applied(request, 2))
            .unwrap();
        let guard = publisher.0.practice.lock().unwrap();
        assert_eq!(
            viewer.pending_practice_request().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(
            viewer.take_practice_response().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(viewer.practice_pending());
        drop(guard);
        // The old split probe would lose the request action at this point.
        let response = viewer.take_practice_response().unwrap().unwrap();
        assert_eq!(response.request, request);
        assert_eq!(response.request.action, action);
        assert_eq!(response.reply.id, id);
        assert!(response.reply.result.is_ok());
        assert!(!viewer.practice_pending());
        assert!(viewer.pending_practice_request().unwrap().is_none());
        assert!(viewer.take_practice_response().unwrap().is_none());
    }
}

#[test]
fn practice_control_actual_channel_pending_stale_ack_and_generation() {
    let (publisher, viewer) = channel();
    assert!(viewer
        .request_practice(PracticeAction::Scrub { target: t(1) })
        .is_err());
    publisher.advertise_practice(Some(cap())).unwrap();
    assert!(viewer
        .request_practice(PracticeAction::Loop {
            start: t(2),
            end: t(1)
        })
        .is_err());
    let id = viewer
        .request_practice(PracticeAction::Scrub {
            target: t(604_800_000_000_000),
        })
        .unwrap();
    let request = publisher.take_practice_request().unwrap().unwrap();
    assert_eq!(request.id, id);
    assert!(publisher.take_practice_request().unwrap().is_none());
    assert!(viewer
        .request_practice(PracticeAction::DisableLoop)
        .is_err());
    viewer.request_pause(true);
    assert!(!viewer.pause_requested());
    assert!(viewer.request_output(vec![]).is_err());
    let mut wrong = applied(request, 2);
    wrong.id += 1;
    assert!(publisher.reply_practice(&wrong).is_err());
    assert!(viewer.practice_pending());
    assert_eq!(viewer.pending_practice_request().unwrap(), Some(request));
    let wrong = applied(request, 1);
    assert!(publisher.reply_practice(&wrong).is_err());
    publisher
        .commit_practice_reply(&applied(request, 2))
        .unwrap();
    assert!(viewer.practice_pending()); // Unconsumed acknowledgement owns the slot.
    assert_eq!(viewer.take_practice_reply().unwrap().unwrap().id, id);
    assert!(!viewer.practice_pending());
    assert_eq!(viewer.practice_capability().unwrap().unwrap().generation, 2);
    let next = viewer
        .request_practice(PracticeAction::DisableLoop)
        .unwrap();
    assert!(next > id);
    let request = publisher.take_practice_request().unwrap().unwrap();
    publisher.reply_practice(&applied(request, 2)).unwrap(); // Mode receipt keeps generation.
    assert!(viewer
        .take_practice_reply()
        .unwrap()
        .unwrap()
        .result
        .is_ok());
}
#[test]
fn practice_control_tls_pause_cancel_and_close_preserve_request_identity() {
    let (publisher, viewer) = channel();
    with_publisher(publisher.clone(), || {
        advertise_practice(Some(cap())).unwrap();
        publish_pause(PauseState::Paused);
        assert!(viewer
            .request_practice(PracticeAction::DisableLoop)
            .is_err());
        publish_pause(PauseState::Running);
        viewer.request_pause(true);
        assert!(viewer
            .request_practice(PracticeAction::DisableLoop)
            .is_err());
        viewer.request_pause(false);
        let id = viewer
            .request_practice(PracticeAction::Scrub { target: t(8) })
            .unwrap();
        let request = take_practice_request().unwrap().unwrap();
        viewer.cancel();
        assert!(commit_practice_reply(&applied(request, 2)).is_err());
        let refusal = viewer.take_practice_reply().unwrap().unwrap();
        assert_eq!(refusal.id, id);
        assert!(refusal.result.is_err());
        assert!(viewer.practice_capability().unwrap().is_none());
        Ok(())
    })
    .unwrap();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        advertise_practice(Some(cap())).unwrap();
        viewer
            .request_practice(PracticeAction::DisableLoop)
            .unwrap();
        Ok(())
    })
    .unwrap();
    assert!(viewer
        .take_practice_reply()
        .unwrap()
        .unwrap()
        .result
        .is_err());
    assert!(viewer.practice_capability().unwrap().is_none());
}
#[test]
fn practice_control_refusal_keeps_generation_and_next_identity() {
    let (publisher, viewer) = channel();
    publisher.advertise_practice(Some(cap())).unwrap();
    let first = viewer
        .request_practice(PracticeAction::Scrub { target: t(20) })
        .unwrap();
    let request = publisher.take_practice_request().unwrap().unwrap();
    publisher
        .reply_practice(&PracticeReply {
            id: request.id,
            generation: request.generation,
            result: Err("audio control ring full".into()),
        })
        .unwrap();
    assert_eq!(
        viewer.take_practice_reply().unwrap().unwrap().result,
        Err("audio control ring full".into())
    );
    assert_eq!(viewer.practice_capability().unwrap(), Some(cap()));
    assert!(
        viewer
            .request_practice(PracticeAction::Scrub { target: t(21) })
            .unwrap()
            > first
    );
}

fn practice_original() -> BmsChart {
    beatkernel_bms::parse(
        "#TITLE Original\n#BPM 60\n#TOTAL 240\n#WAV01 key.wav\n#00011:0101\n#000D1:1E\n#00111:01\n",
        Default::default(),
    )
    .unwrap()
}
fn practice_policy(source: &BmsChart) -> ResolvedPlayPolicy {
    crate::native_judge::NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: beatkernel::time::ClockDomainId(1),
        end: None,
    }
    .resolve_play_policy(
        &crate::play_policy::OriginalGaugeContext::from_source(source),
        crate::play_policy::GaugeSelection::Bms(beatkernel_bms::BmsGaugeKind::Groove),
    )
    .unwrap()
}
fn practice_attempt(
    source: &BmsChart,
    policy: &ResolvedPlayPolicy,
) -> crate::practice_session::PreparedPracticeAttempt {
    crate::practice_session::prepare_attempt(
        source,
        policy,
        &crate::session_launch::SessionLaunch::new(vec!["--chart".into(), "original.bms".into()])
            .unwrap(),
        crate::practice_session::PracticeAttemptConfig {
            start: t(2_000_000_000),
            end: Some(t(5_000_000_000)),
            domain: beatkernel::time::ClockDomainId(1),
            chart_seed: 1,
            capture_limits: None,
        },
    )
    .unwrap()
}
struct PracticeExact;
impl beatkernel::time::ClockMapper for PracticeExact {
    fn map(
        &self,
        from: beatkernel::time::ClockPoint,
        to: beatkernel::time::ClockDomainId,
    ) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> beatkernel::time::ClockMappingQuality {
        beatkernel::time::ClockMappingQuality::Exact
    }
}
fn practice_actual_hit(source: &BmsChart, policy: &ResolvedPlayPolicy) -> RuntimeReport {
    use beatkernel::{
        input::*,
        time::{ClockDomainId, ClockPoint},
        transport::{Rate, Transport},
    };
    let config = crate::native_judge::NativeJudgeConfig {
        early: 0,
        late: 0,
        offset: 0,
        preroll: 0,
        output: ClockDomainId(1),
        end: None,
    };
    let judge = config
        .judge_with_policy(source, source.source.compile().unwrap(), policy)
        .unwrap();
    let (producer, _consumer) = beatkernel::audio::command_queue(8).unwrap();
    let mut runtime = beatkernel::runtime::Runtime::new(
        ClockDomainId(1),
        ClockDomainId(1),
        Transport::new(t(0), t(0), Rate::NORMAL),
        BindingMap::from_bindings([Binding {
            device: DeviceSelector::Any,
            physical: PhysicalControlId::keyboard(7u16),
            game_control: GameControlId(0x11),
        }])
        .unwrap(),
        judge,
        producer,
        Vec::new(),
        0,
    )
    .unwrap();
    runtime.set_processing_clock(beatkernel::runtime::RuntimeProcessingClock::Disabled);
    runtime
        .process_input(
            PhysicalInputEvent::Button(ButtonEvent {
                meta: EventMeta::new(
                    DeviceId(41),
                    ClockPoint {
                        domain: ClockDomainId(1),
                        timestamp: t(0),
                    },
                    71,
                ),
                control: PhysicalControlId::keyboard(7u16),
                state: ButtonState::Down,
            }),
            &PracticeExact,
            ClockPoint {
                domain: ClockDomainId(1),
                timestamp: t(0),
            },
        )
        .unwrap()
}
fn practice_current() -> PlayerSnapshot {
    SESSION.with(|session| session.borrow().as_ref().unwrap().snapshot.clone())
}
fn assert_fresh_practice_member(
    member: &LocalPlayerSnapshot,
    attempt: &crate::practice_session::PreparedPracticeAttempt,
) {
    assert_eq!(member.song_time, Some(attempt.config.start));
    assert_eq!(member.score, ScoreSummary::default());
    assert_eq!(member.bms_score, Some(BmsScoreSummary::default()));
    assert_eq!(member.mine_damage, MineDamageSummary::default());
    assert_eq!(member.gauge, attempt.gauge);
    assert!(member.last_judge.is_none());
    assert!(member.recent_results.is_empty());
    assert_eq!(member.pressed_lanes, 0);
    assert!(member.competition.is_none());
    let chart = member.chart.as_ref().unwrap();
    assert_eq!(
        chart
            .notes
            .iter()
            .map(|note| note.object)
            .collect::<Vec<_>>(),
        attempt
            .judge
            .chart()
            .objects()
            .iter()
            .map(|object| object.id)
            .collect::<Vec<_>>()
    );
    assert!(member.note_progress.as_ref().unwrap().matches_chart(chart));
    for index in 0..chart.notes.len() {
        assert_eq!(
            member.note_progress.as_ref().unwrap().state(index),
            Some(crate::note_progress::NoteState::Pending)
        );
    }
}
#[test]
fn practice_presentation_resets_actual_scores_held_notes_and_keeps_pending_identity() {
    let source = practice_original();
    let policy = practice_policy(&source);
    let attempt = practice_attempt(&source, &policy);
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &source.source.compile().unwrap()).unwrap();
        prepare_native_play_policies(&[(PlayerId(1), &policy)]).unwrap();
        let report = practice_actual_hit(&source, &policy);
        assert_eq!(report.judge_events.len(), 1);
        publish_report(&report).unwrap();
        let before = practice_current();
        assert_eq!(before.players[0].score.hits, 1);
        assert_eq!(before.players[0].bms_score.unwrap().ex_score, 2);
        assert_eq!(before.players[0].pressed_lanes, 1);
        assert_eq!(before.players[0].mine_damage.triggered, 1);
        assert_eq!(
            before.players[0].note_progress.as_ref().unwrap().state(0),
            Some(crate::note_progress::NoteState::Completed)
        );
        publish_saved_competition(
            PlayerId(1),
            vec![GhostSnapshot {
                kind: crate::competition::OpponentKind::Own,
                label: "old.bkr".into(),
                hits: 9,
                misses: 1,
                combo: 3,
                max_combo: 9,
                recorded_until: Some(t(99)),
            }],
        )
        .unwrap();
        publish_section_end(t(1));
        advertise_practice(Some(cap())).unwrap();
        let id = viewer
            .request_practice(PracticeAction::Scrub {
                target: attempt.config.start,
            })
            .unwrap();
        let request = take_practice_request().unwrap().unwrap();
        let staged = prepare_practice_presentation(1, &[(PlayerId(1), &attempt)]).unwrap();
        assert_eq!(practice_current().players[0].score.hits, 1); // Preparation is cold.
        commit_practice_presentation(staged, 2).unwrap();
        let latest = viewer.take_latest().unwrap();
        assert_fresh_practice_member(&latest.players[0], &attempt);
        assert_eq!(latest.score, ScoreSummary::default());
        assert_eq!(latest.bms_score, Some(BmsScoreSummary::default()));
        assert_eq!(latest.pressed_lanes, 0);
        assert!(latest.completed_end.is_none());
        assert!(latest.completed_results.is_none());
        assert_eq!(latest.status, PlayerStatus::Playing);
        assert!(!latest.cancelled);
        assert_eq!(viewer.pending_practice_request().unwrap(), Some(request));
        commit_practice_reply(&applied(request, 2)).unwrap();
        assert_eq!(viewer.take_practice_reply().unwrap().unwrap().id, id);
        let next = prepare_practice_presentation(2, &[(PlayerId(1), &attempt)]).unwrap();
        commit_practice_presentation(next, 3).unwrap(); // Automatic loop, no UI request required.
        assert_fresh_practice_member(&viewer.take_latest().unwrap().players[0], &attempt);
        Ok(())
    })
    .unwrap();
}
#[test]
fn practice_presentation_cohort_invalid_last_member_and_stale_commit_are_atomic() {
    let source = practice_original();
    let policy = practice_policy(&source);
    let attempt = practice_attempt(&source, &policy);
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_local_chart(
            &source,
            &source.source.compile().unwrap(),
            &[PlayerId(3), PlayerId(9)],
        )
        .unwrap();
        prepare_native_play_policies(&[(PlayerId(3), &policy), (PlayerId(9), &policy)]).unwrap();
        let actual = practice_actual_hit(&source, &policy);
        publish_local_reports(&[
            PlayerReport {
                player: PlayerId(3),
                report: actual.clone(),
            },
            PlayerReport {
                player: PlayerId(9),
                report: actual,
            },
        ])
        .unwrap();
        assert_eq!(practice_current().players[1].score.hits, 1);
        for rows in [
            vec![(PlayerId(3), &attempt)],
            vec![(PlayerId(9), &attempt), (PlayerId(3), &attempt)],
            vec![(PlayerId(3), &attempt), (PlayerId(99), &attempt)],
        ] {
            assert!(prepare_practice_presentation(1, &rows).is_err());
            assert_eq!(practice_current().players[0].score.hits, 1);
            assert_eq!(practice_current().players[1].pressed_lanes, 1);
        }
        let mut invalid = practice_attempt(&source, &policy);
        invalid.score.hits = 1;
        assert!(prepare_practice_presentation(
            1,
            &[(PlayerId(3), &attempt), (PlayerId(9), &invalid)]
        )
        .is_err());
        assert_eq!(practice_current().players[0].score.hits, 1);
        let stale =
            prepare_practice_presentation(1, &[(PlayerId(3), &attempt), (PlayerId(9), &attempt)])
                .unwrap();
        let staged =
            prepare_practice_presentation(1, &[(PlayerId(3), &attempt), (PlayerId(9), &attempt)])
                .unwrap();
        assert!(commit_practice_presentation(staged, 1).is_err());
        assert_eq!(practice_current().players[1].score.hits, 1);
        let staged =
            prepare_practice_presentation(1, &[(PlayerId(3), &attempt), (PlayerId(9), &attempt)])
                .unwrap();
        commit_practice_presentation(staged, 2).unwrap();
        assert!(commit_practice_presentation(stale, 3).is_err());
        for member in viewer.take_latest().unwrap().players.iter() {
            assert_fresh_practice_member(member, &attempt);
        }
        Ok(())
    })
    .unwrap();
}
#[test]
fn practice_presentation_headless_and_cancelled_attachment_have_explicit_ownership() {
    let source = practice_original();
    let policy = practice_policy(&source);
    let attempt = practice_attempt(&source, &policy);
    let headless = prepare_practice_presentation(1, &[(PlayerId(1), &attempt)]).unwrap();
    commit_practice_presentation(headless, 2).unwrap();
    let headless = prepare_practice_presentation(1, &[(PlayerId(1), &attempt)]).unwrap();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        publish_chart(&source, &source.source.compile().unwrap()).unwrap();
        prepare_native_play_policies(&[(PlayerId(1), &policy)]).unwrap();
        assert!(commit_practice_presentation(headless, 2).is_err());
        let staged = prepare_practice_presentation(1, &[(PlayerId(1), &attempt)]).unwrap();
        viewer.cancel();
        assert!(commit_practice_presentation(staged, 2).is_err());
        assert_eq!(practice_current().players[0].song_time, None);
        assert_eq!(
            SESSION.with(|session| session.borrow().as_ref().unwrap().practice_generation),
            1
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn practice_presentation_publishes_global_recording_lineage_once_per_boundary() {
    let source = practice_original();
    let policy = practice_policy(&source);
    let global = crate::session_launch::SessionLaunch::new(vec![
        "--chart".into(),
        "original.bms".into(),
        "--record-replay".into(),
        "records/game.bkr".into(),
    ])
    .unwrap();
    let mut member = global
        .for_recording_path("records/game.p9.bkr".into(), "records/game.p9.bkr".into())
        .unwrap();
    let (publisher, viewer) = channel();
    with_publisher(publisher, || {
        pin_native_launch(global.clone()).unwrap();
        let first = viewer.take_native_launch().unwrap().unwrap();
        assert_eq!(first.attempt(), 0);
        assert_eq!(first.args()[3], "records/game.bkr");
        assert!(viewer.take_native_launch().unwrap().is_none());
        publish_local_chart(&source, &source.source.compile().unwrap(), &[PlayerId(9)]).unwrap();
        prepare_native_play_policies(&[(PlayerId(9), &policy)]).unwrap();
        let mut stale = None;
        for generation in 1..=3 {
            let attempt = crate::practice_session::prepare_attempt(
                &source,
                &policy,
                &member,
                crate::practice_session::PracticeAttemptConfig {
                    start: t(2_000_000_000),
                    end: Some(t(5_000_000_000)),
                    domain: beatkernel::time::ClockDomainId(1),
                    chart_seed: 1,
                    capture_limits: crate::native_judge::capture_limits(true, 16384, 128).unwrap(),
                },
            )
            .unwrap();
            let staged =
                prepare_practice_presentation(generation, &[(PlayerId(9), &attempt)]).unwrap();
            if generation == 1 {
                stale = Some(
                    prepare_practice_presentation(generation, &[(PlayerId(9), &attempt)]).unwrap(),
                );
            }
            assert!(viewer.take_native_launch().unwrap().is_none()); // Cold is not publication.
            commit_practice_presentation(staged, generation + 1).unwrap();
            member = attempt.next_launch;
            let published = viewer.take_native_launch().unwrap().unwrap();
            assert_eq!(published.attempt(), generation as u32);
            assert_eq!(published.original_args(), global.original_args());
            assert_eq!(
                published.args()[3],
                format!("records/game.retry{generation}.bkr")
            );
            assert_eq!(
                member.args()[3],
                format!("records/game.p9.retry{generation}.bkr")
            );
            assert!(viewer.take_native_launch().unwrap().is_none());
        }
        assert!(commit_practice_presentation(stale.unwrap(), 5).is_err());
        assert!(viewer.take_native_launch().unwrap().is_none());
        SESSION.with(|session| {
            assert_eq!(
                session
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .native_launch
                    .as_ref()
                    .unwrap()
                    .attempt(),
                3
            );
        });
        Ok(())
    })
    .unwrap();
    // Cleanup preserves final retry lineage rather than rewinding to the pin.
    assert!(viewer.take_native_launch().unwrap().is_none());
}

fn practice_recording_launch(path: &str) -> crate::session_launch::SessionLaunch {
    crate::session_launch::SessionLaunch::new(vec![
        "--chart".into(),
        "original.bms".into(),
        "--record-replay".into(),
        path.into(),
    ])
    .unwrap()
}
fn practice_recording_attempt(
    source: &BmsChart,
    policy: &ResolvedPlayPolicy,
    launch: &crate::session_launch::SessionLaunch,
) -> crate::practice_session::PreparedPracticeAttempt {
    crate::practice_session::prepare_attempt(
        source,
        policy,
        launch,
        crate::practice_session::PracticeAttemptConfig {
            start: t(2_000_000_000),
            end: Some(t(5_000_000_000)),
            domain: beatkernel::time::ClockDomainId(1),
            chart_seed: 1,
            capture_limits: crate::native_judge::capture_limits(true, 16384, 128).unwrap(),
        },
    )
    .unwrap()
}
#[test]
fn practice_applied_identity_survives_cancel_and_visual_poison_without_screen_reset() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let source = practice_original();
    let policy = practice_policy(&source);
    for fault in 0..3 {
        let launch = practice_recording_launch("records/game.bkr");
        let attempt = practice_recording_attempt(&source, &policy, &launch);
        let (publisher, viewer) = channel();
        with_publisher(publisher.clone(), || {
            pin_native_launch(launch.clone()).unwrap();
            let _ = viewer.take_native_launch().unwrap();
            publish_chart(&source, &source.source.compile().unwrap()).unwrap();
            prepare_native_play_policies(&[(PlayerId(1), &policy)]).unwrap();
            SESSION.with(|session| session.borrow_mut().as_mut().unwrap().last_publish = None);
            publish_report(&practice_actual_hit(&source, &policy)).unwrap();
            let mut staged = prepare_practice_presentation(1, &[(PlayerId(1), &attempt)]).unwrap();
            if fault == 0 {
                viewer.cancel();
            } else {
                assert!(catch_unwind(AssertUnwindSafe(|| {
                    let _guard = publisher.0.latest.lock().unwrap();
                    panic!("scripted visual publication poison");
                }))
                .is_err());
            }
            if fault == 2 {
                assert!(catch_unwind(AssertUnwindSafe(|| {
                    let _guard = publisher.0.native_launch.lock().unwrap();
                    panic!("scripted complete metadata swap poison");
                }))
                .is_err());
            }
            apply_practice_identity(&mut staged, 2);
            let applied = viewer.take_native_launch().unwrap().unwrap();
            assert_eq!(applied.attempt(), 1);
            assert_eq!(applied.args()[3], "records/game.retry1.bkr");
            assert_eq!(
                applied.retry().unwrap().args()[3],
                "records/game.retry2.bkr"
            );
            apply_practice_identity(&mut staged, 2);
            assert!(viewer.take_native_launch().unwrap().is_none());
            assert!(commit_practice_presentation(staged, 2).is_err());
            let current = practice_current();
            assert_eq!(current.players[0].score.hits, 1);
            assert_eq!(current.players[0].pressed_lanes, 1);
            assert_eq!(current.players[0].song_time, Some(t(0)));
            SESSION.with(|session| {
                let session = session.borrow();
                let current = session.as_ref().unwrap();
                assert_eq!(current.native_launch.as_ref().unwrap().attempt(), 1);
                assert_eq!(current.practice_generation, 1); // Visual reset did not commit.
            });
            let retained = publisher
                .0
                .latest
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            assert_eq!(retained.as_ref().unwrap().players[0].score.hits, 1);
            assert_eq!(retained.as_ref().unwrap().players[0].song_time, Some(t(0)));
            Ok(())
        })
        .unwrap();
    }
}
#[test]
fn practice_applied_identity_updates_originating_viewer_without_mutating_foreign_attachment() {
    let source = practice_original();
    let policy = practice_policy(&source);
    let launch = practice_recording_launch("records/original.bkr");
    let attempt = practice_recording_attempt(&source, &policy, &launch);
    let (publisher, old_viewer) = channel();
    let mut staged = with_publisher(publisher, || {
        pin_native_launch(launch.clone()).unwrap();
        let _ = old_viewer.take_native_launch().unwrap();
        publish_chart(&source, &source.source.compile().unwrap()).unwrap();
        prepare_native_play_policies(&[(PlayerId(1), &policy)]).unwrap();
        Ok(prepare_practice_presentation(1, &[(PlayerId(1), &attempt)]).unwrap())
    })
    .unwrap();
    let (foreign, foreign_viewer) = channel();
    with_publisher(foreign, || {
        pin_native_launch(practice_recording_launch("records/foreign.bkr")).unwrap();
        apply_practice_identity(&mut staged, 2);
        let actual = old_viewer.take_native_launch().unwrap().unwrap();
        assert_eq!(actual.args()[3], "records/original.retry1.bkr");
        assert_eq!(actual.attempt(), 1);
        let foreign = foreign_viewer.take_native_launch().unwrap().unwrap();
        assert_eq!(foreign.args()[3], "records/foreign.bkr");
        assert_eq!(foreign.attempt(), 0);
        SESSION.with(|session| {
            let session = session.borrow();
            assert_eq!(
                session
                    .as_ref()
                    .unwrap()
                    .native_launch
                    .as_ref()
                    .unwrap()
                    .attempt(),
                0
            );
        });
        assert!(commit_practice_presentation(staged, 2).is_err());
        assert!(practice_current().players.is_empty());
        Ok(())
    })
    .unwrap();
}
