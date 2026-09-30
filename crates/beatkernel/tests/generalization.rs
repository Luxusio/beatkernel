#[allow(dead_code)]
#[path = "../examples/generalization.rs"]
mod fixture;

use beatkernel::{
    chart::ObjectId,
    input::{ButtonState, DeviceId, PhysicalInputEvent, TouchPhase},
    interaction::InteractionState,
    judge::{JudgeEngine, JudgeOutcome, MissReason},
    replay::ReplaySession,
    time::{ClockDomainId, Timestamp},
};

#[test]
fn six_physical_patterns_keep_provenance_and_match_reconstructed_state() {
    let mut fixture = fixture::build_fixture().unwrap();
    let mut mid_checkpoint = None;
    for (index, event) in fixture::fixture_events().into_iter().enumerate() {
        let raw_meta = *event.meta();
        let report = fixture.input(event).unwrap();
        assert_eq!(report.bound_inputs.len(), 1);
        let normalized = report.bound_inputs[0].physical.meta();
        assert_eq!(normalized.source, raw_meta.source);
        assert_eq!(normalized.native, raw_meta.native);
        assert_eq!(normalized.sequence, raw_meta.sequence);
        assert_eq!(
            normalized.original_clock_point.unwrap().domain,
            ClockDomainId(1)
        );
        assert_eq!(normalized.clock_domain, ClockDomainId(2));
        if index == 17 {
            let prefix = fixture.replay_session().unwrap();
            mid_checkpoint = Some((
                prefix.cursor(),
                prefix.stable_hash().unwrap(),
                fixture.runtime.judge().stable_hash().unwrap(),
            ));
        }
    }
    fixture.advance(400).unwrap();
    let mut replay = fixture.replay_session().unwrap();
    let results = replay.results().to_vec();
    assert_eq!(results.len(), 7);
    for id in 1..=7 {
        assert_eq!(
            fixture.runtime.judge().state(ObjectId(id)),
            Some(InteractionState::Completed)
        );
        let result = results
            .iter()
            .find(|event| event.object == ObjectId(id))
            .unwrap();
        assert!(matches!(result.outcome, JudgeOutcome::Hit { .. }));
        let meta = result
            .input
            .expect("sample, not fabricated deadline completion");
        assert_eq!(meta.clock_domain, ClockDomainId(2));
        assert!(meta.original_clock_point.is_some());
        assert_eq!(meta.native.unwrap().timestamp, meta.original_clock_point);
    }
    let final_cursor = replay.cursor();
    let final_hash = replay.stable_hash().unwrap();
    let final_state = fixture.runtime.judge().stable_hash().unwrap();
    assert_eq!(final_state, replay.engine().stable_hash().unwrap());
    let (cursor, hash, state) = mid_checkpoint.unwrap();
    replay.seek_cursor(cursor).unwrap();
    replay.checkpoint().unwrap();
    for _ in 0..2 {
        replay.seek_cursor(cursor).unwrap();
        assert_eq!(replay.stable_hash().unwrap(), hash);
        assert_eq!(replay.engine().stable_hash().unwrap(), state);
        replay.seek_cursor(final_cursor).unwrap();
        assert_eq!(replay.stable_hash().unwrap(), final_hash);
        assert_eq!(replay.engine().stable_hash().unwrap(), final_state);
        assert_eq!(replay.results(), results);
    }
    let header = replay.header().clone();
    let records = replay.records().to_vec();
    let origin = fixture::build_fixture().unwrap();
    let engine = JudgeEngine::from_snapshot(&origin.runtime.judge().snapshot().unwrap()).unwrap();
    let replayed = ReplaySession::from_records(header, engine, records).unwrap();
    assert_eq!(replayed.stable_hash().unwrap(), final_hash);
}

#[test]
fn two_contacts_on_one_surface_ignore_wrong_device_and_contact() {
    let mut fixture = fixture::build_fixture().unwrap();
    for event in [
        fixture::contact(20, 101, 100, 1, 0.0, 0.0, TouchPhase::Down),
        fixture::contact(20, 102, 100, 2, 0.0, 1.0, TouchPhase::Down),
        fixture::contact(20, 101, 200, 3, 0.5, 0.0, TouchPhase::Move),
        fixture::contact(20, 102, 200, 4, 0.5, 1.0, TouchPhase::Move),
        fixture::contact(21, 101, 250, 1, 0.75, 0.0, TouchPhase::Cancel),
        fixture::contact(20, 999, 250, 5, 0.75, 0.0, TouchPhase::Cancel),
    ] {
        fixture.input(event).unwrap();
    }
    assert_eq!(
        fixture.runtime.judge().state(ObjectId(2)),
        Some(InteractionState::Active)
    );
    assert_eq!(
        fixture.runtime.judge().state(ObjectId(3)),
        Some(InteractionState::Active)
    );
    let first = fixture
        .input(fixture::contact(20, 101, 300, 6, 1.0, 0.0, TouchPhase::Up))
        .unwrap();
    assert_eq!(first.judge_events.len(), 1);
    assert_eq!(first.judge_events[0].object, ObjectId(2));
    assert_eq!(first.judge_events[0].input.unwrap().source, DeviceId(20));
    let second = fixture
        .input(fixture::contact(20, 102, 300, 7, 1.0, 1.0, TouchPhase::Up))
        .unwrap();
    assert_eq!(second.judge_events.len(), 1);
    assert_eq!(second.judge_events[0].object, ObjectId(3));
}

#[test]
fn repeated_counts_fresh_press_transitions_and_snapshots_held_owners() {
    let mut fixture = fixture::build_fixture().unwrap();
    fixture
        .input(fixture::button(30, 4, 100, 1, ButtonState::Down))
        .unwrap();
    let snapshot = fixture.runtime.judge().snapshot().unwrap();
    for (time, sequence, state) in [
        (110, 2, ButtonState::Repeat),
        (120, 3, ButtonState::Down),
        (130, 4, ButtonState::Up),
        (140, 5, ButtonState::Down),
    ] {
        let report = fixture
            .input(fixture::button(30, 4, time, sequence, state))
            .unwrap();
        assert!(report
            .judge_events
            .iter()
            .all(|event| event.object != ObjectId(4)));
    }
    assert_eq!(
        fixture.runtime.judge().state(ObjectId(4)),
        Some(InteractionState::Active)
    );
    fixture
        .input(fixture::button(30, 4, 150, 6, ButtonState::Up))
        .unwrap();
    let final_press = fixture
        .input(fixture::button(30, 4, 160, 7, ButtonState::Down))
        .unwrap();
    assert!(final_press
        .judge_events
        .iter()
        .any(|event| event.object == ObjectId(4)
            && matches!(event.outcome, JudgeOutcome::Hit { .. })));
    let mut restored = JudgeEngine::from_snapshot(&snapshot).unwrap();
    let normalized_repeat = final_press.bound_inputs[0].clone();
    // At the checkpoint its owner was already down: another Down remains a
    // duplicate even when restored at a later song timestamp.
    assert!(restored
        .push_input(&normalized_repeat, Timestamp::from_nanos(160))
        .unwrap()
        .iter()
        .all(|event| event.object != ObjectId(4)));
}

#[test]
fn chord_prerequisites_require_trigger_device_and_survive_snapshot_restore() {
    let mut fixture = fixture::build_fixture().unwrap();
    fixture
        .input(fixture::button(41, 51, 0, 1, ButtonState::Down))
        .unwrap();
    fixture
        .input(fixture::button(41, 52, 0, 2, ButtonState::Down))
        .unwrap();
    let failed = fixture
        .input(fixture::button(40, 5, 200, 1, ButtonState::Down))
        .unwrap();
    assert!(failed
        .judge_events
        .iter()
        .all(|event| event.object != ObjectId(5)));
    fixture
        .input(fixture::button(40, 5, 200, 2, ButtonState::Up))
        .unwrap();
    fixture
        .input(fixture::button(40, 51, 200, 3, ButtonState::Down))
        .unwrap();
    fixture
        .input(fixture::button(40, 52, 200, 4, ButtonState::Down))
        .unwrap();
    let snapshot = fixture.runtime.judge().snapshot().unwrap();
    let trigger = fixture
        .input(fixture::button(40, 5, 200, 5, ButtonState::Down))
        .unwrap();
    let result = trigger
        .judge_events
        .iter()
        .find(|event| event.object == ObjectId(5))
        .unwrap();
    assert!(matches!(result.outcome, JudgeOutcome::Hit { .. }));
    assert_eq!(result.input.unwrap().source, DeviceId(40));
    let mut restored = JudgeEngine::from_snapshot(&snapshot).unwrap();
    assert_eq!(
        restored
            .push_input(&trigger.bound_inputs[0], trigger.song_time)
            .unwrap(),
        trigger.judge_events
    );
}

#[test]
fn tracking_gap_or_owner_cancel_reports_explicit_failure() {
    let mut fixture = fixture::build_fixture().unwrap();
    fixture.input(fixture::axis(100, 1, 0.0)).unwrap();
    fixture
        .input(fixture::contact(
            20,
            101,
            100,
            1,
            0.0,
            0.0,
            TouchPhase::Down,
        ))
        .unwrap();
    let cancellation = fixture
        .input(fixture::contact(
            20,
            101,
            200,
            2,
            0.5,
            0.0,
            TouchPhase::Cancel,
        ))
        .unwrap();
    assert!(cancellation
        .judge_events
        .iter()
        .any(|event| event.object == ObjectId(2)
            && event.outcome
                == JudgeOutcome::Miss {
                    reason: MissReason::EarlyRelease
                }));
    let gap = fixture.advance(251).unwrap();
    assert!(gap
        .judge_events
        .iter()
        .any(|event| event.object == ObjectId(1)
            && event.outcome
                == JudgeOutcome::Miss {
                    reason: MissReason::TailTimeout
                }));
}

#[test]
fn wrong_typed_payload_cannot_begin_pose_tracking() {
    let mut fixture = fixture::build_fixture().unwrap();
    let mut wrong = fixture::axis(100, 1, 0.0);
    if let PhysicalInputEvent::Axis(axis) = &mut wrong {
        axis.control = fixture::physical(7);
    }
    let report = fixture.input(wrong).unwrap();
    assert_eq!(report.bound_inputs[0].game_control.0, 7);
    assert_eq!(
        fixture.runtime.judge().state(ObjectId(7)),
        Some(InteractionState::Pending)
    );
    assert!(report
        .judge_events
        .iter()
        .all(|event| event.object != ObjectId(7)));
}

#[test]
fn restore_rejects_changed_evaluator_configuration_without_mutating_target() {
    let three = fixture::build_fixture_with_count(3).unwrap();
    let snapshot = three.runtime.judge().snapshot().unwrap();
    let mut two = fixture::build_fixture_with_count(2).unwrap();
    let before = two.runtime.judge().stable_hash().unwrap();
    assert_eq!(
        two.runtime.judge_mut().restore(&snapshot),
        Err(beatkernel::judge::SnapshotError::ConfigurationMismatch)
    );
    assert_eq!(two.runtime.judge().stable_hash().unwrap(), before);
}

#[allow(dead_code)]
#[path = "../examples/contact_rebind.rs"]
mod contact_fixture;

mod contact_rebind {
    use super::contact_fixture::{build_engine, contact, Policy};
    use beatkernel::{
        chart::ObjectId,
        input::{DeviceId, GameInputEvent, PhysicalInputEvent, TouchPhase},
        interaction::InteractionState,
        judge::{JudgeEngine, JudgeEvent, JudgeOutcome, JudgeStage, MissReason, SnapshotError},
        replay::{ReplayHeader, ReplaySession, REPLAY_VERSION},
        time::{Duration, Timestamp},
    };

    fn after_release(grace: i64) -> Policy {
        Policy::AfterRelease {
            grace: Duration::from_nanos(grace),
        }
    }
    fn push(engine: &mut JudgeEngine, event: GameInputEvent) -> Vec<JudgeEvent> {
        let time = event.physical.meta().timestamp;
        engine.push_input(&event, time).unwrap()
    }
    fn active(engine: &mut JudgeEngine) {
        assert!(push(engine, contact(7, 3, 10, 100, TouchPhase::Down)).is_empty());
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Active));
    }
    fn hit(events: &[JudgeEvent], expected_time: i64) {
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].stage, JudgeStage::Custom(0));
        assert_eq!(events[0].at, Timestamp::from_nanos(expected_time));
        assert!(matches!(events[0].outcome, JudgeOutcome::Hit { .. }));
    }

    #[test]
    fn pending_requires_down_in_head_window_and_region() {
        let mut engine = build_engine(Policy::Locked).unwrap();
        assert!(push(&mut engine, contact(7, 3, 10, 89, TouchPhase::Down)).is_empty());
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Pending));
        assert!(push(&mut engine, contact(7, 3, 10, 90, TouchPhase::Move)).is_empty());
        let mut outside = contact(7, 3, 10, 95, TouchPhase::Down);
        if let PhysicalInputEvent::Touch(event) = &mut outside.physical {
            event.position.x = 2.0;
        }
        assert!(push(&mut engine, outside).is_empty());
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Pending));
        assert!(push(&mut engine, contact(7, 3, 10, 110, TouchPhase::Down)).is_empty());
        hit(
            &push(&mut engine, contact(7, 3, 10, 300, TouchPhase::Up)),
            300,
        );
        let mut earliest = build_engine(Policy::Locked).unwrap();
        assert!(push(&mut earliest, contact(7, 3, 10, 90, TouchPhase::Down)).is_empty());
        hit(
            &push(&mut earliest, contact(7, 3, 10, 290, TouchPhase::Up)),
            290,
        );
    }

    #[test]
    fn locked_release_is_terminal_early_release_with_actual_input_provenance() {
        let mut engine = build_engine(Policy::Locked).unwrap();
        active(&mut engine);
        let mut release = contact(7, 3, 10, 150, TouchPhase::Up);
        release.physical.meta_mut().original_clock_point = Some(beatkernel::time::ClockPoint {
            domain: beatkernel::time::ClockDomainId(99),
            timestamp: Timestamp::from_nanos(-500),
        });
        let normalized_domain = release.physical.meta().clock_domain;
        release.physical.meta_mut().native = Some(beatkernel::input::NativeEventMeta {
            backend: beatkernel::input::BackendId(9),
            code: Some(3),
            timestamp: Some(beatkernel::time::ClockPoint {
                domain: normalized_domain,
                timestamp: Timestamp::from_nanos(150),
            }),
        });
        let meta = *release.physical.meta();
        let events = push(&mut engine, release);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].outcome,
            JudgeOutcome::Miss {
                reason: MissReason::EarlyRelease
            }
        );
        assert_eq!(events[0].input, Some(meta));
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Completed));
        assert!(push(&mut engine, contact(7, 3, 11, 160, TouchPhase::Down)).is_empty());
        assert!(push(&mut engine, contact(7, 3, 11, 300, TouchPhase::Up)).is_empty());
    }

    #[test]
    fn owner_device_surface_and_contact_cannot_be_stolen_or_released_by_others() {
        let mut engine = build_engine(after_release(100)).unwrap();
        active(&mut engine);
        for event in [
            contact(8, 3, 10, 120, TouchPhase::Up),
            contact(7, 4, 10, 130, TouchPhase::Cancel),
            contact(7, 3, 99, 140, TouchPhase::Up),
            contact(7, 3, 99, 150, TouchPhase::Down),
            contact(7, 3, 99, 160, TouchPhase::Cancel),
        ] {
            assert!(push(&mut engine, event).is_empty());
        }
        let events = push(&mut engine, contact(7, 3, 10, 300, TouchPhase::Up));
        hit(&events, 300);
        assert_eq!(events[0].input.unwrap().source, DeviceId(7));
    }

    #[test]
    fn detached_move_cancel_and_wrong_owner_down_do_not_acquire_or_extend_grace() {
        let mut engine = build_engine(after_release(100)).unwrap();
        active(&mut engine);
        assert!(push(&mut engine, contact(7, 3, 10, 150, TouchPhase::Up)).is_empty());
        for event in [
            contact(7, 3, 11, 160, TouchPhase::Move),
            contact(7, 3, 10, 170, TouchPhase::Cancel),
            contact(8, 3, 11, 180, TouchPhase::Down),
            contact(7, 4, 11, 190, TouchPhase::Down),
        ] {
            assert!(push(&mut engine, event).is_empty());
        }
        let mut wrong_destination = contact(7, 3, 11, 200, TouchPhase::Down);
        wrong_destination.game_control = beatkernel::input::GameControlId(2);
        assert!(push(&mut engine, wrong_destination).is_empty());
        assert!(push(&mut engine, contact(7, 3, 11, 250, TouchPhase::Down)).is_empty());
        assert!(push(&mut engine, contact(7, 3, 10, 270, TouchPhase::Up)).is_empty());
        let release = contact(7, 3, 11, 300, TouchPhase::Up);
        let expected_meta = *release.physical.meta();
        let events = push(&mut engine, release);
        hit(&events, 300);
        assert_eq!(events[0].input, Some(expected_meta));
    }

    #[test]
    fn inclusive_grace_boundary_can_reacquire_same_or_new_contact_but_plus_one_expires() {
        for next_contact in [10, 11] {
            let mut engine = build_engine(after_release(20)).unwrap();
            active(&mut engine);
            assert!(push(&mut engine, contact(7, 3, 10, 150, TouchPhase::Up)).is_empty());
            assert!(engine
                .advance_to(Timestamp::from_nanos(170))
                .unwrap()
                .is_empty());
            assert!(push(
                &mut engine,
                contact(7, 3, next_contact, 170, TouchPhase::Down)
            )
            .is_empty());
            hit(
                &push(
                    &mut engine,
                    contact(7, 3, next_contact, 300, TouchPhase::Up),
                ),
                300,
            );
        }
        let mut expired = build_engine(after_release(20)).unwrap();
        active(&mut expired);
        assert!(push(&mut expired, contact(7, 3, 10, 150, TouchPhase::Up)).is_empty());
        assert!(push(&mut expired, contact(7, 3, 11, 169, TouchPhase::Move)).is_empty());
        let events = push(&mut expired, contact(7, 3, 11, 171, TouchPhase::Down));
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].outcome,
            JudgeOutcome::Miss {
                reason: MissReason::TailTimeout
            }
        );
        assert_eq!(
            expired.state(ObjectId(1)),
            Some(InteractionState::Completed)
        );
        assert!(push(&mut expired, contact(7, 3, 11, 300, TouchPhase::Up)).is_empty());
    }

    #[test]
    fn acquired_owner_cancel_is_terminal_rejected_input() {
        let mut engine = build_engine(after_release(20)).unwrap();
        active(&mut engine);
        let cancel = contact(7, 3, 10, 130, TouchPhase::Cancel);
        let expected_meta = *cancel.physical.meta();
        let events = push(&mut engine, cancel);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].outcome,
            JudgeOutcome::Miss {
                reason: MissReason::RejectedInput
            }
        );
        assert_eq!(events[0].input, Some(expected_meta));
        assert!(engine
            .advance_to(Timestamp::from_nanos(400))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn acquired_contact_rejects_out_of_region_and_nonfinite_geometry() {
        for x in [2.0, -0.01, f32::NAN, f32::INFINITY] {
            let mut engine = build_engine(after_release(20)).unwrap();
            active(&mut engine);
            let mut invalid = contact(7, 3, 10, 130, TouchPhase::Move);
            if let PhysicalInputEvent::Touch(event) = &mut invalid.physical {
                event.position.x = x;
            }
            let events = push(&mut engine, invalid);
            assert_eq!(events.len(), 1);
            assert_eq!(
                events[0].outcome,
                JudgeOutcome::Miss {
                    reason: MissReason::RejectedInput
                }
            );
            assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Completed));
            assert!(push(&mut engine, contact(7, 3, 10, 300, TouchPhase::Up)).is_empty());
        }
        let mut engine = build_engine(after_release(20)).unwrap();
        active(&mut engine);
        let mut cancel = contact(7, 3, 10, 130, TouchPhase::Cancel);
        if let PhysicalInputEvent::Touch(event) = &mut cancel.physical {
            event.position.x = f32::NAN;
        }
        let events = push(&mut engine, cancel);
        assert_eq!(
            events[0].outcome,
            JudgeOutcome::Miss {
                reason: MissReason::RejectedInput
            }
        );
    }

    #[test]
    fn tail_window_release_hits_before_detachment_and_time_never_completes_without_up() {
        for policy in [Policy::Locked, after_release(20)] {
            let mut engine = build_engine(policy).unwrap();
            active(&mut engine);
            hit(
                &push(&mut engine, contact(7, 3, 10, 290, TouchPhase::Up)),
                290,
            );
            let mut unreleased = build_engine(policy).unwrap();
            active(&mut unreleased);
            assert!(unreleased
                .advance_to(Timestamp::from_nanos(310))
                .unwrap()
                .is_empty());
            assert_eq!(
                unreleased.state(ObjectId(1)),
                Some(InteractionState::Active)
            );
            let timeout = unreleased.advance_to(Timestamp::from_nanos(311)).unwrap();
            assert_eq!(timeout.len(), 1);
            assert_eq!(
                timeout[0].outcome,
                JudgeOutcome::Miss {
                    reason: MissReason::TailTimeout
                }
            );
            assert_eq!(timeout[0].input, None);
        }
    }

    #[test]
    fn detached_snapshot_restores_owner_release_time_and_policy_configuration() {
        let mut engine = build_engine(after_release(20)).unwrap();
        active(&mut engine);
        assert!(push(&mut engine, contact(7, 3, 10, 150, TouchPhase::Up)).is_empty());
        let snapshot = engine.snapshot().unwrap();
        let detached_hash = engine.stable_hash().unwrap();
        let mut restored = JudgeEngine::from_snapshot(&snapshot).unwrap();
        assert_eq!(restored.stable_hash().unwrap(), detached_hash);
        for event in [
            contact(8, 3, 11, 160, TouchPhase::Down),
            contact(7, 3, 11, 170, TouchPhase::Down),
            contact(7, 3, 11, 300, TouchPhase::Up),
        ] {
            assert_eq!(push(&mut restored, event.clone()), push(&mut engine, event));
            assert_eq!(
                restored.stable_hash().unwrap(),
                engine.stable_hash().unwrap()
            );
        }
        for policy in [Policy::Locked, after_release(21)] {
            let mut incompatible = build_engine(policy).unwrap();
            let before = incompatible.stable_hash().unwrap();
            assert_eq!(
                incompatible.restore(&snapshot),
                Err(SnapshotError::ConfigurationMismatch)
            );
            assert_eq!(incompatible.stable_hash().unwrap(), before);
        }
    }

    #[test]
    fn different_owner_and_acquired_contact_are_part_of_future_state_hash() {
        let mut hashes = Vec::new();
        for (device, surface, id) in [(7, 3, 10), (8, 3, 10), (7, 4, 10), (7, 3, 11)] {
            let mut engine = build_engine(after_release(20)).unwrap();
            assert!(push(
                &mut engine,
                contact(device, surface, id, 100, TouchPhase::Down)
            )
            .is_empty());
            hashes.push(engine.stable_hash().unwrap());
        }
        for left in 0..hashes.len() {
            for right in left + 1..hashes.len() {
                assert_ne!(hashes[left], hashes[right]);
            }
        }
    }

    #[test]
    fn replay_detached_checkpoint_and_reverse_seek_reconstruct_exact_rebind_results() {
        let first = contact(7, 3, 10, 100, TouchPhase::Down);
        let header = ReplayHeader {
            version: REPLAY_VERSION,
            chart_identity: b"contact-fixture/v1".to_vec(),
            rules_identity: b"release-rebind/v1".to_vec(),
            options: vec![],
            seed: 0,
            normalized_clock: first.physical.meta().clock_domain,
        };
        let mut replay =
            ReplaySession::new(header, build_engine(after_release(20)).unwrap()).unwrap();
        assert!(replay
            .push_input(first, Timestamp::from_nanos(100))
            .unwrap()
            .is_empty());
        assert!(replay
            .push_input(
                contact(7, 3, 10, 150, TouchPhase::Up),
                Timestamp::from_nanos(150)
            )
            .unwrap()
            .is_empty());
        replay.checkpoint().unwrap();
        let detached_hash = replay.engine().stable_hash().unwrap();
        assert!(replay
            .push_input(
                contact(7, 3, 11, 170, TouchPhase::Down),
                Timestamp::from_nanos(170)
            )
            .unwrap()
            .is_empty());
        assert!(replay
            .push_input(
                contact(7, 3, 10, 280, TouchPhase::Up),
                Timestamp::from_nanos(280)
            )
            .unwrap()
            .is_empty());
        hit(
            &replay
                .push_input(
                    contact(7, 3, 11, 300, TouchPhase::Up),
                    Timestamp::from_nanos(300),
                )
                .unwrap(),
            300,
        );
        let results = replay.results().to_vec();
        let completed_hash = replay.engine().stable_hash().unwrap();
        for _ in 0..2 {
            replay.seek_cursor(2).unwrap();
            assert!(replay.results().is_empty());
            assert_eq!(replay.engine().stable_hash().unwrap(), detached_hash);
            replay.seek_cursor(5).unwrap();
            assert_eq!(replay.results(), results);
            assert_eq!(replay.engine().stable_hash().unwrap(), completed_hash);
            replay.seek(Timestamp::from_nanos(150)).unwrap();
            let mut linear = build_engine(after_release(20)).unwrap();
            push(&mut linear, contact(7, 3, 10, 100, TouchPhase::Down));
            push(&mut linear, contact(7, 3, 10, 150, TouchPhase::Up));
            linear.advance_to(Timestamp::from_nanos(150)).unwrap();
            assert_eq!(
                replay.engine().stable_hash().unwrap(),
                linear.stable_hash().unwrap()
            );
        }
    }

    #[test]
    fn nonpositive_grace_is_rejected_at_setup() {
        for grace in [0, -1, i64::MIN] {
            assert!(build_engine(after_release(grace)).is_err());
        }
    }
}
