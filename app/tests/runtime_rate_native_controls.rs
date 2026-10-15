//! UI/host correlation tests. Copied event DTOs are mailbox evidence, not PCM proof.
#![cfg(all(feature = "desktop", not(target_arch = "wasm32")))]

use beatkernel::{
    audio::{PracticeRateEvent, PracticeRateOutcome, PracticeRateRefusal},
    runtime::RuntimeReport,
    time::Timestamp,
    transport::Rate,
};
use beatkernel_bms_runtime::{
    competition::ScoreSummary,
    live_output_control::OutputCapability,
    local_runtime::PlayerReport,
    native_gameplay::NativeGameplayResult,
    native_gameplay_host::{
        NativeGameplayDiagnostic, NativeGameplayHost, NativeScoreHost, PauseState,
    },
    player::{self, PlayerPublisher, PlayerViewer},
    practice_control::*,
    settings::SettingsHost,
};

fn rate(n: i64, d: u64) -> Rate {
    Rate::new(n, d).unwrap()
}

fn capability() -> PracticeTransactionCapability {
    PracticeTransactionCapability {
        practice: PracticeCapability {
            generation: 7,
            min_target: Timestamp::ZERO,
            max_target: Timestamp::from_nanos(1_000_000_000),
        },
        rate: PracticeRateState {
            revision: 11,
            rate: Rate::NORMAL,
        },
    }
}

fn channel() -> (PlayerPublisher, PlayerViewer) {
    let (publisher, viewer) = player::channel();
    publisher
        .advertise_practice_transactions(Some(capability()))
        .unwrap();
    (publisher, viewer)
}

fn copied_event(request: PracticeTransactionRequest) -> PracticeRateEvent {
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
        iteration: 3,
        request_revision: expected.revision,
        applied_revision: expected.revision + 1,
        physical_frame: 9_007_199_254_740_993,
        playback_frame: 9_007_199_254_740_000,
        active_elapsed_nanos: 123_456_789_012_345_678_901,
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

fn copied_reply(
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

#[test]
fn explicit_runtime_advertisement_preserves_legacy_capability_without_optimistic_rate() {
    let (publisher, viewer) = player::channel();
    assert_eq!(viewer.practice_transaction_capability().unwrap(), None);
    assert!(viewer.request_practice_rate(rate(3, 2)).is_err());
    publisher
        .advertise_practice(Some(capability().practice))
        .unwrap();
    assert!(viewer.request_practice_rate(rate(3, 2)).is_err());
    publisher
        .advertise_practice_transactions(Some(capability()))
        .unwrap();
    assert_eq!(
        viewer.practice_capability().unwrap(),
        Some(capability().practice)
    );
    assert_eq!(
        viewer.practice_transaction_capability().unwrap(),
        Some(capability())
    );
    viewer.request_practice_rate(rate(6, 4)).unwrap();
    assert!(viewer.practice_pending());
    assert_eq!(
        viewer.practice_transaction_capability().unwrap(),
        Some(capability())
    );
    assert!(viewer
        .take_practice_transaction_response()
        .unwrap()
        .is_none());
}

#[test]
fn rate_region_and_original_restart_share_one_pending_slot_and_identity_allocator() {
    let (publisher, viewer) = channel();
    let id = viewer.request_practice_rate(rate(3, 2)).unwrap();
    let first = viewer.pending_practice_transaction().unwrap().unwrap();
    assert_eq!(
        first,
        PracticeTransactionRequest::Rate {
            id,
            generation: 7,
            expected: capability().rate,
            requested_rate: rate(3, 2),
        }
    );
    assert!(viewer.request_practice_rate(rate(2, 1)).is_err());
    assert!(viewer.request_practice_original_restart().is_err());
    assert!(viewer
        .request_practice(PracticeAction::Scrub {
            target: Timestamp::ZERO
        })
        .is_err());
    assert!(publisher.take_practice_request().unwrap().is_none());
    assert_eq!(
        publisher.take_practice_transaction(true).unwrap(),
        Some(first)
    );
    assert!(publisher.take_practice_transaction(true).unwrap().is_none());
    let reply = copied_reply(first, copied_event(first));
    publisher.commit_practice_transaction_reply(&reply).unwrap();
    assert!(
        viewer.practice_pending(),
        "unconsumed result still owns the slot"
    );
    assert!(viewer.request_practice_original_restart().is_err());
    assert_eq!(
        viewer.take_practice_transaction_response().unwrap(),
        Some(PracticeTransactionResponse {
            request: first,
            reply
        })
    );
    let restart = viewer.request_practice_original_restart().unwrap();
    assert_eq!(restart, id + 1);
    assert_eq!(
        publisher.take_practice_transaction(true).unwrap(),
        Some(PracticeTransactionRequest::RestartOriginal {
            id: restart,
            generation: 7,
            expected: PracticeRateState {
                revision: 12,
                rate: rate(3, 2)
            },
        })
    );
}

#[test]
fn successful_rate_reply_updates_only_actual_rate_and_revision_and_is_consumed_once() {
    let (publisher, viewer) = channel();
    viewer.request_practice_rate(rate(3, 2)).unwrap();
    let request = publisher.take_practice_transaction(true).unwrap().unwrap();
    let event = copied_event(request);
    let reply = copied_reply(request, event);
    publisher.commit_practice_transaction_reply(&reply).unwrap();
    let actual = viewer.practice_transaction_capability().unwrap().unwrap();
    assert_eq!(actual.practice, capability().practice);
    assert_eq!(
        actual.rate,
        PracticeRateState {
            revision: 12,
            rate: rate(3, 2)
        }
    );
    assert!(publisher.commit_practice_transaction_reply(&reply).is_err());
    let response = viewer
        .take_practice_transaction_response()
        .unwrap()
        .unwrap();
    assert_eq!(response.request, request);
    assert_eq!(response.reply, reply);
    assert!(!viewer.practice_pending());
    assert!(viewer
        .take_practice_transaction_response()
        .unwrap()
        .is_none());
    assert!(publisher
        .commit_practice_transaction_reply(&response.reply)
        .is_err());
}

#[test]
fn wrong_generation_event_and_outer_reply_cannot_complete_or_mutate_pending_request() {
    let (publisher, viewer) = channel();
    viewer.request_practice_rate(rate(3, 2)).unwrap();
    let request = publisher.take_practice_transaction(true).unwrap().unwrap();
    let valid = copied_reply(request, copied_event(request));
    let mut wrong_outer = valid.clone();
    if let PracticeTransactionReply::Rate { generation, .. } = &mut wrong_outer {
        *generation += 1;
    }
    let mut event = copied_event(request);
    event.generation += 1;
    for invalid in [wrong_outer, copied_reply(request, event)] {
        assert!(publisher
            .commit_practice_transaction_reply(&invalid)
            .is_err());
        assert_eq!(
            viewer.practice_transaction_capability().unwrap(),
            Some(capability())
        );
        assert_eq!(
            viewer.pending_practice_transaction().unwrap(),
            Some(request)
        );
        assert!(viewer
            .take_practice_transaction_response()
            .unwrap()
            .is_none());
    }
    publisher.commit_practice_transaction_reply(&valid).unwrap();
    assert_eq!(
        viewer
            .take_practice_transaction_response()
            .unwrap()
            .unwrap()
            .reply,
        valid
    );
}

#[test]
fn outer_success_with_audio_refusal_preserves_actual_rate_and_inner_refusal_facts() {
    let (publisher, viewer) = channel();
    viewer.request_practice_rate(rate(3, 2)).unwrap();
    let request = publisher.take_practice_transaction(true).unwrap().unwrap();
    let mut event = copied_event(request);
    event.applied_revision = capability().rate.revision;
    event.applied_rate = capability().rate.rate;
    event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity);
    let reply = copied_reply(request, event);
    publisher.commit_practice_transaction_reply(&reply).unwrap();
    assert_eq!(
        viewer.practice_transaction_capability().unwrap(),
        Some(capability())
    );
    let response = viewer
        .take_practice_transaction_response()
        .unwrap()
        .unwrap();
    assert_eq!(response.reply, reply);
    assert!(matches!(
        response.reply,
        PracticeTransactionReply::Rate {
            result: Ok(PracticeRateEvent {
                outcome: PracticeRateOutcome::Refused(PracticeRateRefusal::BgmCapacity),
                ..
            }),
            ..
        }
    ));
    assert!(!viewer.practice_pending());
}

#[test]
fn equal_reduced_rate_noop_preserves_generation_and_advances_only_revision() {
    let (publisher, viewer) = channel();
    viewer.request_practice_rate(rate(12, 12)).unwrap();
    let request = publisher.take_practice_transaction(true).unwrap().unwrap();
    let event = copied_event(request);
    assert_eq!(event.outcome, PracticeRateOutcome::Noop);
    publisher
        .commit_practice_transaction_reply(&copied_reply(request, event))
        .unwrap();
    assert_eq!(
        viewer.practice_transaction_capability().unwrap(),
        Some(PracticeTransactionCapability {
            rate: PracticeRateState {
                revision: 12,
                rate: Rate::NORMAL
            },
            ..capability()
        })
    );
    assert_eq!(
        viewer
            .take_practice_transaction_response()
            .unwrap()
            .unwrap()
            .request,
        request
    );
}

#[test]
fn paused_rate_waits_for_real_active_owner_and_resume_is_allowed_through_player_guard() {
    let (publisher, viewer) = channel();
    player::with_publisher(publisher.clone(), || {
        player::publish_pause(PauseState::Paused);
        viewer.request_pause(true);
        assert!(viewer.pause_requested());
        viewer.request_practice_rate(rate(3, 2)).unwrap();
        let pending = viewer.pending_practice_transaction().unwrap().unwrap();
        assert!(publisher
            .take_practice_transaction(false)
            .unwrap()
            .is_none());
        assert!(publisher.take_practice_transaction(true).unwrap().is_none());
        viewer.request_pause(false);
        assert!(
            !viewer.pause_requested(),
            "a pending rate must not deadlock resume"
        );
        assert!(publisher
            .take_practice_transaction(false)
            .unwrap()
            .is_none());
        assert!(publisher.take_practice_transaction(true).unwrap().is_none());
        player::publish_pause(PauseState::Running);
        assert_eq!(publisher.take_practice_transaction(false).unwrap(), None);
        assert_eq!(
            publisher.take_practice_transaction(true).unwrap(),
            Some(pending)
        );
        assert_eq!(
            viewer.practice_transaction_capability().unwrap(),
            Some(capability())
        );
        publisher
            .commit_practice_transaction_reply(&copied_reply(pending, copied_event(pending)))
            .unwrap();
        assert!(viewer
            .take_practice_transaction_response()
            .unwrap()
            .is_some());
        Ok(())
    })
    .unwrap();
}

#[test]
fn region_and_original_restart_keep_pause_and_output_conflict_guards() {
    let (publisher, viewer) = channel();
    viewer.request_pause(true);
    assert!(viewer.request_practice_original_restart().is_err());
    assert!(viewer
        .request_practice(PracticeAction::DisableLoop)
        .is_err());
    viewer.request_pause(false);
    let id = viewer.request_practice_original_restart().unwrap();
    viewer.request_pause(true);
    assert!(
        !viewer.pause_requested(),
        "restart keeps its existing pause exclusion"
    );
    assert!(matches!(
        publisher.take_practice_transaction(true).unwrap(),
        Some(PracticeTransactionRequest::RestartOriginal { id: actual, .. }) if actual == id
    ));

    let (publisher, viewer) = channel();
    let args = vec![
        "--alsa".into(),
        "fixture-device".into(),
        "--buffer-frames".into(),
        "128".into(),
        "--period-frames".into(),
        "32".into(),
    ];
    publisher
        .advertise_output(Some(OutputCapability {
            host: SettingsHost::Linux,
            current_args: args.clone(),
        }))
        .unwrap();
    viewer.request_output(args).unwrap();
    assert!(viewer.request_practice_rate(rate(3, 2)).is_err());
    assert!(viewer.request_practice_original_restart().is_err());
    assert!(!viewer.practice_pending());
}

#[test]
fn revocation_and_cancel_resolve_original_request_as_refusal_without_new_owner_or_rate() {
    for cancelled in [false, true] {
        let (publisher, viewer) = channel();
        viewer.request_practice_rate(rate(3, 2)).unwrap();
        let request = publisher.take_practice_transaction(true).unwrap().unwrap();
        if cancelled {
            viewer.cancel();
        } else {
            publisher.advertise_practice_transactions(None).unwrap();
        }
        assert!(viewer.practice_transaction_capability().unwrap().is_none());
        assert!(publisher.take_practice_transaction(true).unwrap().is_none());
        let response = viewer
            .take_practice_transaction_response()
            .unwrap()
            .unwrap();
        assert_eq!(response.request, request);
        assert!(matches!(
            response.reply,
            PracticeTransactionReply::Rate { result: Err(_), .. }
        ));
        assert!(!viewer.practice_pending());
        assert!(viewer
            .take_practice_transaction_response()
            .unwrap()
            .is_none());
        assert!(viewer.request_practice_rate(rate(2, 1)).is_err());
        assert!(publisher
            .commit_practice_transaction_reply(&copied_reply(request, copied_event(request)))
            .is_err());
    }
}

#[test]
fn tls_functions_forward_to_attached_player_mailbox_and_do_not_create_another_slot() {
    let (publisher, viewer) = player::channel();
    player::with_publisher(publisher.clone(), || {
        player::advertise_practice_transactions(Some(capability())).unwrap();
        viewer.request_practice_rate(rate(3, 2)).unwrap();
        assert!(player::take_practice_transaction(false).unwrap().is_none());
        let request = player::take_practice_transaction(true).unwrap().unwrap();
        assert!(publisher.take_practice_transaction(true).unwrap().is_none());
        let reply = copied_reply(request, copied_event(request));
        player::commit_practice_transaction_reply(&reply).unwrap();
        assert_eq!(
            viewer.take_practice_transaction_response().unwrap(),
            Some(PracticeTransactionResponse { request, reply })
        );
        Ok(())
    })
    .unwrap();
    assert!(player::take_practice_transaction(true).unwrap().is_none());
}

#[derive(Default)]
struct HostSpy {
    advertised: Vec<Option<PracticeTransactionCapability>>,
    active: Vec<bool>,
    request: Option<PracticeTransactionRequest>,
    replies: Vec<PracticeTransactionReply>,
    identity_resets: usize,
}
impl NativeGameplayHost for HostSpy {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn advertise_practice_transactions(
        &mut self,
        cap: Option<PracticeTransactionCapability>,
    ) -> NativeGameplayResult<()> {
        self.advertised.push(cap);
        Ok(())
    }
    fn take_practice_transaction(
        &mut self,
        active: bool,
    ) -> NativeGameplayResult<Option<PracticeTransactionRequest>> {
        self.active.push(active);
        Ok(if active { self.request.take() } else { None })
    }
    fn commit_practice_transaction_reply(
        &mut self,
        reply: &PracticeTransactionReply,
    ) -> NativeGameplayResult<()> {
        self.replies.push(reply.clone());
        Ok(())
    }
    fn apply_practice_identity(&mut self, _: &mut player::PreparedPracticePresentation, _: u64) {
        self.identity_resets += 1;
    }
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn publish_local_reports(&mut self, _: &[PlayerReport]) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}

#[test]
fn score_host_statically_forwards_applied_noop_and_refused_controls_without_resetting_score() {
    for requested_rate in [rate(3, 2), Rate::NORMAL] {
        for refused in [false, true] {
            let request = PracticeTransactionRequest::Rate {
                id: 5,
                generation: 7,
                expected: capability().rate,
                requested_rate,
            };
            let mut event = copied_event(request);
            if refused {
                event.applied_revision = 11;
                event.applied_rate = Rate::NORMAL;
                event.outcome = PracticeRateOutcome::Refused(PracticeRateRefusal::Ended);
            }
            let reply = copied_reply(request, event);
            let mut spy = HostSpy {
                request: Some(request),
                ..Default::default()
            };
            let mut score = ScoreSummary {
                hits: 37,
                misses: 2,
                combo: 9,
                max_combo: 25,
                grades: [(1, 31), (2, 6)].into_iter().collect(),
                ..Default::default()
            };
            let before = score.clone();
            {
                let mut host = NativeScoreHost::new(&mut spy, &mut score);
                host.advertise_practice_transactions(Some(capability()))
                    .unwrap();
                assert!(host.take_practice_transaction(false).unwrap().is_none());
                assert_eq!(host.take_practice_transaction(true).unwrap(), Some(request));
                host.commit_practice_transaction_reply(&reply).unwrap();
            }
            assert_eq!(score, before);
            assert_eq!(spy.advertised, vec![Some(capability())]);
            assert_eq!(spy.active, vec![false, true]);
            assert_eq!(spy.replies, vec![reply]);
            assert_eq!(spy.identity_resets, 0);
        }
    }
}
