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
