#[path = "../examples/generalization/pose_interaction.rs"]
mod pose_interaction;

use beatkernel::{
    chart::*,
    input::*,
    interaction::*,
    judge::*,
    replay::{ReplayHeader, ReplaySession, REPLAY_VERSION},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use pose_interaction::DerivedPoseEvaluator;

fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn profile() -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(7),
            early: Duration::from_nanos(10),
            late: Duration::from_nanos(10),
        }],
        Duration::ZERO,
    )
    .unwrap()
}
fn chart() -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(100).unwrap(),
        end: Some(Beat::new(300).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    compile(&source).unwrap()
}
fn engine() -> JudgeEngine {
    JudgeEngine::new(
        chart(),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(7),
            evaluator: Box::new(DerivedPoseEvaluator),
        }],
        profile(),
    )
    .unwrap()
}
fn identity() -> Quaternion {
    Quaternion {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    }
}
fn pose(time: i64, p: f32, orientation: Quaternion) -> GameInputEvent {
    let original = ClockPoint {
        domain: ClockDomainId(99),
        timestamp: ts(time - 1_000),
    };
    let mut meta = EventMeta::new(
        DeviceId(70),
        ClockPoint {
            domain: ClockDomainId(2),
            timestamp: ts(time),
        },
        time as u64,
    );
    meta.original_clock_point = Some(original);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(17),
        code: Some(700),
        timestamp: Some(original),
    });
    GameInputEvent {
        game_control: GameControlId(7),
        physical: PhysicalInputEvent::Pose(PoseEvent {
            meta,
            control: PhysicalControlId::HidUsage {
                usage_page: 1,
                usage: 7,
            },
            position: Position3 { x: p, y: p, z: p },
            orientation,
        }),
    }
}
fn push(engine: &mut JudgeEngine, input: GameInputEvent) -> Vec<JudgeEvent> {
    engine
        .push_input(&input, input.physical.meta().timestamp)
        .unwrap()
}
fn start(engine: &mut JudgeEngine) {
    assert!(push(engine, pose(100, 0.0, identity())).is_empty());
    assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Active));
}
fn assert_result(events: &[JudgeEvent], at: i64, outcome: JudgeOutcome, input: Option<EventMeta>) {
    assert_eq!(
        events,
        &[JudgeEvent {
            object: ObjectId(1),
            stage: JudgeStage::Custom(0),
            outcome,
            at: ts(at),
            input
        }]
    );
}
fn hit(delta: i64) -> JudgeOutcome {
    JudgeOutcome::Hit {
        grade: JudgeGrade(7),
        delta: Duration::from_nanos(delta),
    }
}
fn miss(reason: MissReason) -> JudgeOutcome {
    JudgeOutcome::Miss { reason }
}

#[test]
fn derived_pose_linear_path_hits_only_on_final_input_with_exact_provenance() {
    let mut engine = engine();
    start(&mut engine);
    assert!(push(&mut engine, pose(200, 0.5, identity())).is_empty());
    assert!(push(&mut engine, pose(290, 0.95, identity())).is_empty());
    assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Active));
    let final_input = pose(300, 1.0, identity());
    assert_result(
        &push(&mut engine, final_input.clone()),
        300,
        hit(0),
        Some(*final_input.physical.meta()),
    );
    assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Completed));
    assert!(push(&mut engine, pose(301, 1.0, identity())).is_empty());
    assert!(engine.advance_to(ts(900)).unwrap().is_empty());
}

#[test]
fn derived_pose_start_window_and_tail_late_limit_are_inclusive() {
    for time in [90, 100, 110] {
        let mut engine = engine();
        assert!(push(&mut engine, pose(time, 0.0, identity())).is_empty());
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Active));
        assert!(push(&mut engine, pose(200, 0.5, identity())).is_empty());
        let final_input = pose(310, 1.0, identity());
        assert_result(
            &push(&mut engine, final_input.clone()),
            310,
            hit(10),
            Some(*final_input.physical.meta()),
        );
    }
    let mut early = engine();
    assert!(push(&mut early, pose(89, 0.0, identity())).is_empty());
    assert_eq!(early.state(ObjectId(1)), Some(InteractionState::Pending));
    assert!(early.advance_to(ts(110)).unwrap().is_empty());
    assert_result(
        &early.advance_to(ts(111)).unwrap(),
        111,
        miss(MissReason::HeadTimeout),
        None,
    );
}

#[test]
fn derived_pose_gap_and_tail_deadlines_expire_strictly_and_advance_never_hits() {
    let mut inclusive = engine();
    start(&mut inclusive);
    assert!(inclusive.advance_to(ts(250)).unwrap().is_empty());
    assert!(push(&mut inclusive, pose(250, 0.75, identity())).is_empty());
    let end = pose(300, 1.0, identity());
    assert_result(
        &push(&mut inclusive, end.clone()),
        300,
        hit(0),
        Some(*end.physical.meta()),
    );
    let mut gap = engine();
    start(&mut gap);
    assert_result(
        &gap.advance_to(ts(251)).unwrap(),
        251,
        miss(MissReason::TailTimeout),
        None,
    );
    assert!(push(&mut gap, pose(300, 1.0, identity())).is_empty());
    let mut tail = engine();
    start(&mut tail);
    assert!(push(&mut tail, pose(200, 0.5, identity())).is_empty());
    assert!(tail.advance_to(ts(300)).unwrap().is_empty());
    assert!(tail.advance_to(ts(310)).unwrap().is_empty());
    assert_eq!(tail.state(ObjectId(1)), Some(InteractionState::Active));
    assert_result(
        &tail.advance_to(ts(311)).unwrap(),
        311,
        miss(MissReason::TailTimeout),
        None,
    );
}

fn invalid_orientations() -> Vec<Quaternion> {
    vec![
        Quaternion {
            w: 0.0,
            ..identity()
        },
        Quaternion {
            w: 2.0,
            ..identity()
        },
        Quaternion {
            x: f32::NAN,
            ..identity()
        },
        Quaternion {
            w: f32::INFINITY,
            ..identity()
        },
        Quaternion {
            x: 0.6,
            y: 0.0,
            z: 0.0,
            w: 0.8,
        },
    ]
}

#[test]
fn derived_pose_pending_invalid_geometry_orientation_and_payload_do_not_acquire() {
    let mut inputs: Vec<_> = invalid_orientations()
        .into_iter()
        .map(|q| pose(100, 0.0, q))
        .collect();
    inputs.extend([
        pose(100, f32::NAN, identity()),
        pose(100, f32::INFINITY, identity()),
        pose(100, 0.01, identity()),
    ]);
    let mut wrong = pose(100, 0.0, identity());
    wrong.physical = PhysicalInputEvent::Button(ButtonEvent {
        meta: *wrong.physical.meta(),
        control: PhysicalControlId::keyboard(7),
        state: ButtonState::Down,
    });
    inputs.push(wrong);
    for input in inputs {
        let mut engine = engine();
        assert!(push(&mut engine, input).is_empty());
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Pending));
        start(&mut engine);
    }
}

#[test]
fn derived_pose_active_bad_owner_sample_emits_one_rejection_with_original_metadata() {
    let mut inputs: Vec<_> = invalid_orientations()
        .into_iter()
        .map(|q| pose(200, 0.5, q))
        .collect();
    inputs.extend([
        pose(200, f32::NAN, identity()),
        pose(200, f32::INFINITY, identity()),
        pose(200, 0.51, identity()),
    ]);
    for input in inputs {
        let mut engine = engine();
        start(&mut engine);
        let meta = *input.physical.meta();
        assert_result(
            &push(&mut engine, input),
            200,
            miss(MissReason::RejectedInput),
            Some(meta),
        );
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Completed));
        assert!(push(&mut engine, pose(300, 1.0, identity())).is_empty());
        assert!(engine.advance_to(ts(400)).unwrap().is_empty());
    }
}

#[test]
fn derived_pose_orientation_cone_is_inclusive_and_quaternion_sign_is_equivalent() {
    for sign in [-1.0, 1.0] {
        for w in [0.9f32, 1.0] {
            let q = Quaternion {
                x: (1.0 - w * w).sqrt() * sign,
                y: 0.0,
                z: 0.0,
                w: w * sign,
            };
            let mut engine = engine();
            assert!(push(&mut engine, pose(100, 0.0, q)).is_empty());
            assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Active));
            assert!(push(&mut engine, pose(200, 0.5, q)).is_empty());
            let final_input = pose(300, 1.0, q);
            assert_result(
                &push(&mut engine, final_input.clone()),
                300,
                hit(0),
                Some(*final_input.physical.meta()),
            );
        }
    }
    let w = 0.8999f32;
    let q = Quaternion {
        x: (1.0 - w * w).sqrt(),
        y: 0.0,
        z: 0.0,
        w,
    };
    let mut engine = engine();
    assert!(push(&mut engine, pose(100, 0.0, q)).is_empty());
    assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Pending));
}

#[test]
fn derived_pose_normalization_tolerance_has_two_sides_and_spatial_limit_is_inclusive() {
    for (norm_squared, accepted) in [
        (0.9991f32, true),
        (1.0009, true),
        (0.9989, false),
        (1.0011, false),
    ] {
        let mut engine = engine();
        let q = Quaternion {
            w: norm_squared.sqrt(),
            ..identity()
        };
        assert!(push(&mut engine, pose(100, 0.0, q)).is_empty());
        assert_eq!(
            engine.state(ObjectId(1)),
            Some(if accepted {
                InteractionState::Active
            } else {
                InteractionState::Pending
            })
        );
    }
    for (offset, accepted) in [(0.001f32, true), (0.0010001, false)] {
        let mut engine = engine();
        let mut input = pose(100, 0.0, identity());
        if let PhysicalInputEvent::Pose(sample) = &mut input.physical {
            sample.position.x = offset;
        }
        assert!(push(&mut engine, input).is_empty());
        assert_eq!(
            engine.state(ObjectId(1)),
            Some(if accepted {
                InteractionState::Active
            } else {
                InteractionState::Pending
            })
        );
    }
}

#[test]
fn derived_pose_foreign_device_full_physical_identity_logical_control_and_payload_are_ignored() {
    let mut engine = engine();
    start(&mut engine);
    let mut foreign_device = pose(120, f32::NAN, identity());
    foreign_device.physical.meta_mut().source = DeviceId(71);
    let mut foreign_page = pose(130, f32::NAN, identity());
    if let PhysicalInputEvent::Pose(event) = &mut foreign_page.physical {
        event.control = PhysicalControlId::HidUsage {
            usage_page: 2,
            usage: 7,
        };
    }
    let mut foreign_usage = pose(140, f32::NAN, identity());
    if let PhysicalInputEvent::Pose(event) = &mut foreign_usage.physical {
        event.control = PhysicalControlId::HidUsage {
            usage_page: 1,
            usage: 8,
        };
    }
    let mut foreign_logical = pose(150, f32::NAN, identity());
    foreign_logical.game_control = GameControlId(8);
    let mut wrong_payload = pose(160, 0.3, identity());
    wrong_payload.physical = PhysicalInputEvent::Axis(AxisEvent {
        meta: *wrong_payload.physical.meta(),
        control: PhysicalControlId::HidUsage {
            usage_page: 1,
            usage: 7,
        },
        value: f32::NAN,
        mode: AxisMode::Absolute,
    });
    for input in [
        foreign_device,
        foreign_page,
        foreign_usage,
        foreign_logical,
        wrong_payload,
    ] {
        assert!(push(&mut engine, input).is_empty());
        assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Active));
    }
    assert!(push(&mut engine, pose(200, 0.5, identity())).is_empty());
    let final_input = pose(300, 1.0, identity());
    assert_result(
        &push(&mut engine, final_input.clone()),
        300,
        hit(0),
        Some(*final_input.physical.meta()),
    );
}

#[test]
fn derived_pose_rejects_absent_zero_and_reversed_object_ranges() {
    let profile = profile();
    for end in [None, Some(ts(100)), Some(ts(99))] {
        let mut object = chart().objects()[0].clone();
        object.time.end = end;
        assert_eq!(
            DerivedPoseEvaluator.validate(&object, &profile),
            Err(JudgeError::InvalidObjectRange {
                object: ObjectId(1)
            })
        );
    }
}

#[test]
fn derived_pose_active_engine_snapshot_restores_owner_and_sample_gap() {
    let mut original = engine();
    start(&mut original);
    assert!(push(&mut original, pose(200, 0.5, identity())).is_empty());
    let snapshot = original.snapshot().unwrap();
    let active_hash = original.stable_hash().unwrap();
    for _ in 0..2 {
        let mut restored = JudgeEngine::from_snapshot(&snapshot).unwrap();
        assert_eq!(restored.stable_hash().unwrap(), active_hash);
        let mut foreign = pose(220, f32::NAN, identity());
        foreign.physical.meta_mut().source = DeviceId(71);
        assert!(push(&mut restored, foreign).is_empty());
        let input = pose(300, 1.0, identity());
        assert_result(
            &push(&mut restored, input.clone()),
            300,
            hit(0),
            Some(*input.physical.meta()),
        );
        let mut deadline = JudgeEngine::from_snapshot(&snapshot).unwrap();
        assert!(deadline.advance_to(ts(310)).unwrap().is_empty());
        assert_result(
            &deadline.advance_to(ts(311)).unwrap(),
            311,
            miss(MissReason::TailTimeout),
            None,
        );
    }
    let input = pose(300, 1.0, identity());
    let expected = push(&mut original, input.clone());
    original.restore(&snapshot).unwrap();
    assert_eq!(original.stable_hash().unwrap(), active_hash);
    assert_eq!(push(&mut original, input), expected);
}

#[test]
fn derived_pose_public_snapshot_bytes_capture_configuration_owner_and_mutable_samples() {
    let profile = profile();
    let object = chart().objects()[0].clone();
    let begin = |object: &TimedObject, control| {
        DerivedPoseEvaluator.begin(
            object,
            &BeginContext {
                control,
                profile: &profile,
            },
        )
    };
    let pending = begin(&object, GameControlId(7));
    let pending_bytes = pending.snapshot_bytes().unwrap();
    assert!(!pending_bytes.is_empty());
    assert_eq!(
        pending.snapshot_clone().unwrap().snapshot_bytes().unwrap(),
        pending_bytes
    );
    assert_ne!(
        begin(&object, GameControlId(8)).snapshot_bytes().unwrap(),
        pending_bytes
    );
    let mut changed = object.clone();
    changed.time.end = Some(ts(301));
    assert_ne!(
        begin(&changed, GameControlId(7)).snapshot_bytes().unwrap(),
        pending_bytes
    );
    let mut bytes = vec![];
    for (time, p, source, q) in [
        (100, 0.0, 70, identity()),
        (100, 0.0, 71, identity()),
        (101, 0.0, 70, identity()),
        (
            100,
            0.0,
            70,
            Quaternion {
                w: -1.0,
                ..identity()
            },
        ),
        (100, 0.0001, 70, identity()),
    ] {
        let mut active = begin(&object, GameControlId(7));
        let mut input = pose(time, p, q);
        input.physical.meta_mut().source = DeviceId(source);
        let context = InteractionContext {
            song_time: ts(time),
            profile: &profile,
            policy: &WindowJudgePolicy,
        };
        assert!(active.on_input(&input, &context).results.is_empty());
        assert_eq!(active.state(), InteractionState::Active);
        let active_bytes = active.snapshot_bytes().unwrap();
        assert_ne!(active_bytes, pending_bytes);
        assert_eq!(
            active.snapshot_clone().unwrap().snapshot_bytes().unwrap(),
            active_bytes
        );
        bytes.push(active_bytes);
    }
    for left in 0..bytes.len() {
        for right in left + 1..bytes.len() {
            assert_ne!(bytes[left], bytes[right]);
        }
    }
}

#[test]
fn derived_pose_replay_active_checkpoint_reverse_and_forward_preserve_literal_result() {
    let header = ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"derived-pose-test/v1".to_vec(),
        rules_identity: b"fixed-derived-pose/v1".to_vec(),
        options: vec![],
        seed: 0,
        normalized_clock: ClockDomainId(2),
    };
    let mut replay = ReplaySession::new(header, engine()).unwrap();
    assert!(replay
        .push_input(pose(100, 0.0, identity()), ts(100))
        .unwrap()
        .is_empty());
    assert!(replay
        .push_input(pose(200, 0.5, identity()), ts(200))
        .unwrap()
        .is_empty());
    replay.checkpoint().unwrap();
    let active_hash = replay.engine().stable_hash().unwrap();
    let final_input = pose(300, 1.0, identity());
    assert_result(
        &replay.push_input(final_input.clone(), ts(300)).unwrap(),
        300,
        hit(0),
        Some(*final_input.physical.meta()),
    );
    let results = replay.results().to_vec();
    let completed_hash = replay.engine().stable_hash().unwrap();
    for _ in 0..2 {
        replay.seek_cursor(2).unwrap();
        assert!(replay.results().is_empty());
        assert_eq!(
            replay.engine().state(ObjectId(1)),
            Some(InteractionState::Active)
        );
        assert_eq!(replay.engine().stable_hash().unwrap(), active_hash);
        replay.seek_cursor(3).unwrap();
        assert_result(
            replay.results(),
            300,
            hit(0),
            Some(*final_input.physical.meta()),
        );
        assert_eq!(replay.results(), results);
        assert_eq!(replay.engine().stable_hash().unwrap(), completed_hash);
    }
}
