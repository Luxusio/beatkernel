#[allow(dead_code)]
#[path = "../examples/generalization.rs"]
mod fixture;

use beatkernel::{
    chart::ObjectId,
    input::{
        AxisMode, BackendId, ButtonState, DeviceId, EventMeta, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent, Quaternion, TouchPhase,
    },
    interaction::InteractionState,
    judge::{JudgeEngine, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeStage, MissReason},
    replay::{ReplayOperation, ReplaySession, REPLAY_VERSION},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

const MODES: [AxisMode; 2] = [AxisMode::Absolute, AxisMode::Relative];

fn ts(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}

fn metadata(device: u64, control: u32, time: i64, sequence: u64) -> EventMeta {
    let original = ClockPoint {
        domain: ClockDomainId(1),
        timestamp: ts(time),
    };
    EventMeta {
        source: DeviceId(device),
        timestamp: ts(1_000_000_000 + time),
        clock_domain: ClockDomainId(2),
        sequence,
        native: Some(NativeEventMeta {
            backend: BackendId(9),
            code: Some(control),
            timestamp: Some(original),
        }),
        original_clock_point: Some(original),
    }
}

fn hit(id: u64, time: i64, device: u64, control: u32, sequence: u64) -> JudgeEvent {
    JudgeEvent {
        object: ObjectId(id),
        stage: if id == 8 {
            JudgeStage::Instant
        } else {
            JudgeStage::Custom(0)
        },
        at: ts(time),
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: Duration::ZERO,
        },
        input: Some(metadata(device, control, time, sequence)),
    }
}

fn expected_hits() -> Vec<JudgeEvent> {
    vec![
        hit(1, 300, 10, 1, 3),
        hit(2, 300, 20, 2, 6),
        hit(3, 300, 20, 2, 7),
        hit(4, 150, 30, 4, 6),
        hit(5, 200, 40, 5, 3),
        hit(6, 300, 50, 6, 4),
        hit(7, 300, 60, 7, 3),
        hit(8, 200, 50, 8, 3),
    ]
}

fn assert_hits(events: &[JudgeEvent]) {
    let mut sorted = events.to_vec();
    sorted.sort_by_key(|event| event.object.0);
    assert_eq!(sorted, expected_hits());
}

fn for_object(events: &[JudgeEvent], id: u64) -> Vec<JudgeEvent> {
    events
        .iter()
        .filter(|event| event.object == ObjectId(id))
        .copied()
        .collect()
}

fn assert_recorded_object(fixture: &fixture::Fixture, id: u64, expected: &[JudgeEvent]) {
    let replay = fixture.replay_session().unwrap();
    assert_eq!(for_object(replay.results(), id), expected);
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        fixture.runtime.judge().stable_hash().unwrap()
    );
}

// A complete Runtime pass supplies the positive oracle for each named style.
// Every input record must retain the literal clock mapping and original payload.
fn complete(mode: AxisMode) -> fixture::Fixture {
    let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
    let header = fixture.recorder.header();
    assert_eq!(header.version, REPLAY_VERSION);
    assert_eq!(header.chart_identity, b"six-pattern-combinations/v2");
    assert_eq!(
        header.rules_identity,
        b"tracking-repeated-composite-instant-derived-pose/v2"
    );
    let mode_name = if mode == AxisMode::Absolute {
        "absolute"
    } else {
        "relative"
    };
    assert_eq!(header.options, format!("tolerance=0.001;gap=150ns;count=3;same_device=true;axis_mode={mode_name};pointer_mode=relative;pose=unit-diagonal-identity-cone/v1;pose_abs_w_min=0.9;pose_norm_squared_tolerance=0.001;pointer_button_control=8").into_bytes());
    assert_eq!(header.seed, 19);
    assert_eq!(header.normalized_clock, ClockDomainId(2));
    let mut live = Vec::new();
    for raw in fixture::six_pattern_events(mode) {
        let incoming = *raw.meta();
        let control = incoming.native.unwrap().code.unwrap();
        let time = incoming.timestamp.as_nanos();
        let expected_meta = metadata(incoming.source.0, control, time, incoming.sequence);
        let mut expected_physical = raw.clone();
        *expected_physical.meta_mut() = expected_meta;
        let report = fixture.input(raw).unwrap();
        assert_eq!(report.song_time, ts(time));
        assert_eq!(report.bound_inputs.len(), 1);
        let bound = &report.bound_inputs[0];
        assert_eq!(bound.game_control.0, control);
        assert_eq!(bound.physical, expected_physical);
        let physical_control = match &bound.physical {
            PhysicalInputEvent::Button(event) => event.control,
            PhysicalInputEvent::Axis(event) => event.control,
            PhysicalInputEvent::Touch(event) => event.control,
            PhysicalInputEvent::Pointer(event) => event.control,
            PhysicalInputEvent::Pose(event) => event.control,
            _ => panic!("six-pattern fixture requires its five typed payload families"),
        };
        assert_eq!(
            physical_control,
            PhysicalControlId::Native {
                backend: BackendId(9),
                code: control,
            }
        );
        let record = fixture.recorder.records().last().unwrap();
        assert_eq!(record.song_time, ts(time));
        assert_eq!(record.operation, ReplayOperation::Input(bound.clone()));
        live.extend(report.judge_events);
    }
    live.extend(fixture.advance(400).unwrap().judge_events);
    assert_hits(&live);
    for id in 1..=8 {
        assert_eq!(
            fixture.runtime.judge().state(ObjectId(id)),
            Some(InteractionState::Completed)
        );
    }
    let replay = fixture.replay_session().unwrap();
    assert_hits(replay.results());
    assert_eq!(replay.results(), live);
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        fixture.runtime.judge().stable_hash().unwrap()
    );
    fixture
}

fn axis(mode: AxisMode, time: i64, sequence: u64, value: f32) -> PhysicalInputEvent {
    let mut event = fixture::axis(time, sequence, value);
    if let PhysicalInputEvent::Axis(axis) = &mut event {
        axis.mode = mode;
    }
    event
}

#[test]
fn sdvx_like_absolute_and_relative_axes_keep_owner_and_expire_sample_gaps() {
    for mode in MODES {
        complete(mode);
        let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
        fixture.input(axis(mode, 100, 1, 0.0)).unwrap();
        let mut foreign = axis(mode, 200, 2, 0.5);
        foreign.meta_mut().source = DeviceId(11);
        assert!(for_object(&fixture.input(foreign).unwrap().judge_events, 1).is_empty());
        assert_eq!(
            fixture.runtime.judge().state(ObjectId(1)),
            Some(InteractionState::Active)
        );
        let expected = JudgeEvent {
            object: ObjectId(1),
            stage: JudgeStage::Custom(0),
            outcome: JudgeOutcome::Miss {
                reason: MissReason::TailTimeout,
            },
            at: ts(251),
            input: None,
        };
        assert_eq!(
            for_object(&fixture.advance(251).unwrap().judge_events, 1),
            [expected]
        );
        assert_recorded_object(&fixture, 1, &[expected]);
    }
}

#[test]
fn arcaea_like_dual_contacts_cannot_steal_or_complete_the_other_path() {
    for mode in MODES {
        complete(mode);
        let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
        for event in [
            fixture::contact(20, 101, 100, 1, 0.0, 0.0, TouchPhase::Down),
            fixture::contact(20, 102, 100, 2, 0.0, 1.0, TouchPhase::Down),
            fixture::contact(21, 101, 150, 1, 0.25, 0.0, TouchPhase::Cancel),
            fixture::contact(20, 999, 150, 3, 0.25, 1.0, TouchPhase::Up),
            fixture::contact(20, 101, 200, 4, 0.5, 0.0, TouchPhase::Move),
            fixture::contact(20, 102, 200, 5, 0.5, 1.0, TouchPhase::Move),
        ] {
            let report = fixture.input(event).unwrap();
            assert!(for_object(&report.judge_events, 2).is_empty());
            assert!(for_object(&report.judge_events, 3).is_empty());
        }
        let first = fixture
            .input(fixture::contact(20, 101, 300, 6, 1.0, 0.0, TouchPhase::Up))
            .unwrap();
        assert_eq!(for_object(&first.judge_events, 2), [hit(2, 300, 20, 2, 6)]);
        assert!(for_object(&first.judge_events, 3).is_empty());
        assert_eq!(
            fixture.runtime.judge().state(ObjectId(3)),
            Some(InteractionState::Active)
        );
        let second = fixture
            .input(fixture::contact(20, 102, 300, 7, 1.0, 1.0, TouchPhase::Up))
            .unwrap();
        assert_eq!(for_object(&second.judge_events, 3), [hit(3, 300, 20, 2, 7)]);
        assert_recorded_object(&fixture, 2, &[hit(2, 300, 20, 2, 6)]);
        assert_recorded_object(&fixture, 3, &[hit(3, 300, 20, 2, 7)]);
    }
}

#[test]
fn taiko_like_roll_counts_fresh_downs_and_ignores_repeat_and_duplicate_down() {
    for mode in MODES {
        complete(mode);
        let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
        for (time, sequence, state) in [
            (100, 1, ButtonState::Down),
            (110, 2, ButtonState::Repeat),
            (120, 3, ButtonState::Down),
            (130, 4, ButtonState::Up),
            (140, 5, ButtonState::Down),
            (150, 6, ButtonState::Up),
        ] {
            assert!(for_object(
                &fixture
                    .input(fixture::button(30, 4, time, sequence, state))
                    .unwrap()
                    .judge_events,
                4
            )
            .is_empty());
        }
        assert_eq!(
            fixture.runtime.judge().state(ObjectId(4)),
            Some(InteractionState::Active)
        );
        let final_press = fixture
            .input(fixture::button(30, 4, 160, 7, ButtonState::Down))
            .unwrap();
        let expected = hit(4, 160, 30, 4, 7);
        assert_eq!(for_object(&final_press.judge_events, 4), [expected]);
        assert!(for_object(
            &fixture
                .input(fixture::button(30, 4, 170, 8, ButtonState::Down))
                .unwrap()
                .judge_events,
            4
        )
        .is_empty());
        assert_recorded_object(&fixture, 4, &[expected]);
    }
}

#[test]
fn gitadora_like_chord_requires_same_device_held_frets_and_fresh_trigger() {
    for mode in MODES {
        complete(mode);
        let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
        for event in [
            fixture::button(41, 51, 0, 1, ButtonState::Down),
            fixture::button(41, 52, 0, 2, ButtonState::Down),
            fixture::button(40, 5, 200, 1, ButtonState::Down),
            fixture::button(40, 51, 200, 2, ButtonState::Down),
            fixture::button(40, 52, 200, 3, ButtonState::Down),
            fixture::button(40, 5, 200, 4, ButtonState::Repeat),
            fixture::button(40, 5, 200, 5, ButtonState::Down),
            fixture::button(40, 5, 200, 6, ButtonState::Up),
        ] {
            assert!(for_object(&fixture.input(event).unwrap().judge_events, 5).is_empty());
        }
        let trigger = fixture
            .input(fixture::button(40, 5, 200, 7, ButtonState::Down))
            .unwrap();
        assert_eq!(
            for_object(&trigger.judge_events, 5),
            [hit(5, 200, 40, 5, 7)]
        );
        assert_recorded_object(&fixture, 5, &[hit(5, 200, 40, 5, 7)]);
    }
}

#[test]
fn osu_like_pointer_trajectory_and_separate_button_have_independent_outcomes() {
    for mode in MODES {
        let complete = complete(mode);
        let pointer_records: Vec<_> = complete
            .recorder
            .records()
            .iter()
            .filter_map(|record| match &record.operation {
                ReplayOperation::Input(input) if input.game_control.0 == 6 => Some(input),
                _ => None,
            })
            .collect();
        assert_eq!(pointer_records.len(), 3);
        for (input, sequence, delta) in [
            (pointer_records[0], 1, 0.0),
            (pointer_records[1], 2, 0.5),
            (pointer_records[2], 4, 0.5),
        ] {
            let PhysicalInputEvent::Pointer(pointer) = &input.physical else {
                panic!("pointer payload required")
            };
            assert_eq!(pointer.meta.sequence, sequence);
            assert_eq!((pointer.position.x, pointer.position.y), (delta, delta));
        }
        let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
        for event in [
            fixture::pointer(100, 1, 0.0),
            fixture::pointer(200, 2, 0.5),
            fixture::pointer(300, 4, 0.5),
        ] {
            fixture.input(event).unwrap();
        }
        fixture.advance(400).unwrap();
        assert_recorded_object(&fixture, 6, &[hit(6, 300, 50, 6, 4)]);
        assert_recorded_object(
            &fixture,
            8,
            &[JudgeEvent {
                object: ObjectId(8),
                stage: JudgeStage::Instant,
                at: ts(300),
                outcome: JudgeOutcome::Miss {
                    reason: MissReason::HeadTimeout,
                },
                input: None,
            }],
        );
    }
}

#[test]
fn vr_like_orientation_policy_rejects_owned_misalignment_and_preserves_q_sign() {
    for mode in MODES {
        let completed = complete(mode);
        let poses: Vec<_> = completed
            .recorder
            .records()
            .iter()
            .filter_map(|record| match &record.operation {
                ReplayOperation::Input(input) => match &input.physical {
                    PhysicalInputEvent::Pose(pose) => Some(pose),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(poses.len(), 3);
        for (pose, sequence, position, w) in [
            (poses[0], 1, 0.0, 1.0),
            (poses[1], 2, 0.5, 1.0),
            (poses[2], 3, 1.0, -1.0),
        ] {
            assert_eq!(pose.meta, metadata(60, 7, sequence as i64 * 100, sequence));
            assert_eq!(
                (pose.position.x, pose.position.y, pose.position.z),
                (position, position, position)
            );
            assert_eq!(
                pose.orientation,
                Quaternion {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w
                }
            );
        }
        let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
        fixture.input(fixture::pose(100, 1, 0.0)).unwrap();
        let mut foreign = fixture::pose(150, 2, 0.25);
        foreign.meta_mut().source = DeviceId(61);
        if let PhysicalInputEvent::Pose(pose) = &mut foreign {
            pose.orientation.w = 0.0;
        }
        assert!(for_object(&fixture.input(foreign).unwrap().judge_events, 7).is_empty());
        assert_eq!(
            fixture.runtime.judge().state(ObjectId(7)),
            Some(InteractionState::Active)
        );
        let mut misaligned = fixture::pose(200, 2, 0.5);
        if let PhysicalInputEvent::Pose(pose) = &mut misaligned {
            pose.orientation = Quaternion {
                x: 1.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            };
        }
        let expected = JudgeEvent {
            object: ObjectId(7),
            stage: JudgeStage::Custom(0),
            at: ts(200),
            outcome: JudgeOutcome::Miss {
                reason: MissReason::RejectedInput,
            },
            input: Some(metadata(60, 7, 200, 2)),
        };
        assert_eq!(
            for_object(&fixture.input(misaligned).unwrap().judge_events, 7),
            [expected]
        );
        assert!(for_object(
            &fixture
                .input(fixture::pose(300, 3, 1.0))
                .unwrap()
                .judge_events,
            7
        )
        .is_empty());
        fixture.advance(400).unwrap();
        assert_recorded_object(&fixture, 7, &[expected]);
    }
}

#[test]
fn active_checkpoint_restores_all_eight_objects_and_literal_results_in_both_modes() {
    for mode in MODES {
        let mut fixture = fixture::build_six_pattern_fixture(mode).unwrap();
        let events = fixture::six_pattern_events(mode);
        let mut live = Vec::new();
        for event in events.iter().take(8) {
            live.extend(fixture.input(event.clone()).unwrap().judge_events);
        }
        assert!(live.is_empty());
        let states = [
            InteractionState::Active,
            InteractionState::Active,
            InteractionState::Active,
            InteractionState::Active,
            InteractionState::Active,
            InteractionState::Active,
            InteractionState::Active,
            InteractionState::Pending,
        ];
        for (index, state) in states.iter().enumerate() {
            assert_eq!(
                fixture.runtime.judge().state(ObjectId(index as u64 + 1)),
                Some(*state)
            );
        }
        let prefix = fixture.replay_session().unwrap();
        let prefix_cursor = prefix.cursor();
        let prefix_hash = prefix.stable_hash().unwrap();
        let prefix_engine_hash = fixture.runtime.judge().stable_hash().unwrap();
        let snapshot = fixture.runtime.judge().snapshot().unwrap();
        let mut restored = JudgeEngine::from_snapshot(&snapshot).unwrap();
        assert_eq!(restored.stable_hash().unwrap(), prefix_engine_hash);
        let mut restored_results = Vec::new();
        for event in events.into_iter().skip(8) {
            let report = fixture.input(event).unwrap();
            restored_results.extend(
                restored
                    .push_input(&report.bound_inputs[0], report.song_time)
                    .unwrap(),
            );
            live.extend(report.judge_events);
        }
        restored_results.extend(restored.advance_to(ts(400)).unwrap());
        live.extend(fixture.advance(400).unwrap().judge_events);
        assert_hits(&live);
        assert_hits(&restored_results);
        let final_engine_hash = fixture.runtime.judge().stable_hash().unwrap();
        assert_eq!(restored.stable_hash().unwrap(), final_engine_hash);
        let mut replay = fixture.replay_session().unwrap();
        let final_cursor = replay.cursor();
        let final_hash = replay.stable_hash().unwrap();
        replay.seek_cursor(prefix_cursor).unwrap();
        replay.checkpoint().unwrap();
        for _ in 0..2 {
            replay.seek_cursor(prefix_cursor).unwrap();
            assert!(replay.results().is_empty());
            assert_eq!(replay.stable_hash().unwrap(), prefix_hash);
            assert_eq!(replay.engine().stable_hash().unwrap(), prefix_engine_hash);
            for (index, state) in states.iter().enumerate() {
                assert_eq!(
                    replay.engine().state(ObjectId(index as u64 + 1)),
                    Some(*state)
                );
            }
            replay.seek_cursor(final_cursor).unwrap();
            assert_hits(replay.results());
            assert_eq!(replay.stable_hash().unwrap(), final_hash);
            assert_eq!(replay.engine().stable_hash().unwrap(), final_engine_hash);
            for id in 1..=8 {
                assert_eq!(
                    replay.engine().state(ObjectId(id)),
                    Some(InteractionState::Completed)
                );
            }
        }
        let origin = fixture::build_six_pattern_fixture(mode).unwrap();
        let fresh =
            JudgeEngine::from_snapshot(&origin.runtime.judge().snapshot().unwrap()).unwrap();
        let rebuilt = ReplaySession::from_records(
            replay.header().clone(),
            fresh,
            replay.records().iter().cloned(),
        )
        .unwrap();
        assert_hits(rebuilt.results());
        assert_eq!(rebuilt.stable_hash().unwrap(), final_hash);
    }
}
