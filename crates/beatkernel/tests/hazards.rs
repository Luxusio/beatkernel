//! Deferred shared-judge hazard fixtures. Application damage, gauge, audio and
//! BMS runtime integration are deliberately outside this component's evidence.
use beatkernel::{
    chart::*,
    input::*,
    interaction::{HoldEvaluator, InstantEvaluator, InteractionState},
    judge::*,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn profile(offset: i64) -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(7),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::from_nanos(offset),
    )
    .unwrap()
}
fn chart(notes: bool) -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    if notes {
        for (id, start, end, interaction) in
            [(1, 100, None, 1), (2, 200, Some(300), 2), (3, 400, None, 1)]
        {
            source.objects.push(SourceObject {
                id: ObjectId(id),
                start: Beat::new(start).unwrap(),
                end: end.map(|value| Beat::new(value).unwrap()),
                interaction: InteractionId(interaction),
                visual: VisualId(0),
                audio: None,
                metadata: ObjectMetadata::default(),
            });
        }
    }
    source.compile().unwrap()
}
fn rules() -> Vec<Rule> {
    vec![
        Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(InstantEvaluator),
        },
        Rule {
            interaction: InteractionId(2),
            control: GameControlId(1),
            evaluator: Box::new(HoldEvaluator),
        },
    ]
}
fn judge(contacts: bool, notes: bool, offset: i64) -> JudgeEngine {
    if contacts {
        JudgeEngine::new_with_contacts(chart(notes), rules(), profile(offset)).unwrap()
    } else {
        JudgeEngine::new(chart(notes), rules(), profile(offset)).unwrap()
    }
}
fn marker(id: u64, at: i64, control: u32, value: u64) -> HazardMarker {
    HazardMarker {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(control),
        value,
    }
}
fn timeline(markers: Vec<HazardMarker>) -> HazardTimeline {
    HazardTimeline::new(markers, 100).unwrap()
}
fn event(
    id: u64,
    at: i64,
    control: u32,
    value: u64,
    outcome: HazardOutcome,
    input: Option<EventMeta>,
) -> HazardEvent {
    HazardEvent {
        id: HazardId(id),
        at: ts(at),
        control: GameControlId(control),
        value,
        outcome,
        input,
    }
}
fn meta(source: u64) -> EventMeta {
    let mut value = EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(u32::MAX),
            timestamp: ts(9_007_199_254_740_993),
        },
        u64::MAX,
    );
    value.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(8),
        timestamp: ts(-31),
    });
    value.native = Some(NativeEventMeta {
        backend: BackendId(0x57494e),
        code: Some(u32::MAX),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(9),
            timestamp: ts(i64::MIN),
        }),
    });
    value
}
fn physical(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(42),
        code,
    }
}
fn button(source: u64, code: u32, control: u32, state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: meta(source),
            control: physical(code),
            state,
        }),
    }
}
fn touch(source: u64, code: u32, control: u32, contact: u64, phase: TouchPhase) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: meta(source),
            control: physical(code),
            contact: ContactId(contact),
            phase,
            position: Position2 { x: -0.0, y: 45.5 },
            pressure: Some(0.75),
        }),
    }
}
fn axis(control: u32) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(control),
        physical: PhysicalInputEvent::Axis(AxisEvent {
            meta: meta(17),
            control: physical(9),
            value: -0.0,
            mode: AxisMode::Relative,
        }),
    }
}

#[test]
fn timeline_validates_caller_budget_unique_full_width_ids_and_stable_declaration_ties() {
    assert!(HazardTimeline::new(vec![], 0).unwrap().markers().is_empty());
    assert!(
        HazardTimeline::new(vec![], usize::MAX)
            .unwrap()
            .markers()
            .is_empty()
    );
    assert_eq!(
        HazardTimeline::new(vec![marker(0, 0, 0, 0)], 0).unwrap_err(),
        HazardError::Capacity
    );
    let declared = vec![
        marker(u64::MAX, 10, u32::MAX, u64::MAX),
        marker(0, i64::MIN, 0, 0),
        marker(8, 10, 1, 2),
        marker(3, -1, 7, 3),
        marker(7, 10, 0, 4),
        marker(1, i64::MAX, 1, 5),
    ];
    let snapshot = declared.clone();
    let ordered = HazardTimeline::new(declared, 6).unwrap();
    assert_eq!(
        ordered.markers(),
        [
            snapshot[1],
            snapshot[3],
            snapshot[0],
            snapshot[2],
            snapshot[4],
            snapshot[5]
        ]
    );
    assert_eq!(
        HazardTimeline::new(snapshot.clone(), 5).unwrap_err(),
        HazardError::Capacity
    );
    for second in [
        marker(u64::MAX, -1, 1, 0),
        marker(u64::MAX, 10, u32::MAX, u64::MAX),
    ] {
        assert_eq!(
            HazardTimeline::new(vec![snapshot[0], second], 2).unwrap_err(),
            HazardError::DuplicateId {
                id: HazardId(u64::MAX)
            }
        );
    }
    // Equal control and time are legal: distinct hazard IDs each produce an outcome.
    let mut engine = judge(false, false, 0);
    engine
        .configure_hazards(timeline(vec![marker(2, 0, 1, 3), marker(1, 0, 1, 4)]))
        .unwrap();
    let down = button(1, 4, 1, ButtonState::Down);
    assert!(engine.push_input(&down, ts(0)).unwrap().is_empty());
    assert_eq!(
        engine.hazard_events(),
        [
            event(2, 0, 1, 3, HazardOutcome::Triggered, Some(meta(1))),
            event(1, 0, 1, 4, HazardOutcome::Triggered, Some(meta(1)))
        ]
    );
}

#[test]
fn input_uses_previous_ownership_before_boundary_and_updated_ownership_at_boundary_once() {
    let mut engine = judge(false, false, 0);
    engine
        .configure_hazards(timeline(vec![
            marker(5, 20, 1, 5),
            marker(9, 10, 1, 9),
            marker(8, 9, 1, 8),
            marker(7, 0, 1, 7),
            marker(6, -1, 1, 6),
            marker(10, 10, 2, 10),
        ]))
        .unwrap();
    let down = button(u64::MAX, u32::MAX, 1, ButtonState::Down);
    let original = down.clone();
    engine.push_input(&down, ts(0)).unwrap();
    assert_eq!(
        engine.hazard_events(),
        [
            event(6, -1, 1, 6, HazardOutcome::Avoided, None),
            event(7, 0, 1, 7, HazardOutcome::Triggered, Some(meta(u64::MAX)))
        ]
    );
    let up = button(u64::MAX, u32::MAX, 1, ButtonState::Up);
    engine.push_input(&up, ts(10)).unwrap();
    assert_eq!(
        engine.hazard_events(),
        [
            event(8, 9, 1, 8, HazardOutcome::Triggered, None),
            event(9, 10, 1, 9, HazardOutcome::Avoided, Some(meta(u64::MAX))),
            event(10, 10, 2, 10, HazardOutcome::Avoided, Some(meta(u64::MAX)))
        ]
    );
    engine.push_input(&up, ts(10)).unwrap();
    assert!(
        engine.hazard_events().is_empty(),
        "a successful empty operation replaces the old report"
    );
    engine.advance_to(ts(20)).unwrap();
    assert_eq!(
        engine.hazard_events(),
        [event(5, 20, 1, 5, HazardOutcome::Avoided, None)]
    );
    engine.push_input(&down, ts(20)).unwrap();
    assert!(
        engine.hazard_events().is_empty(),
        "a later equal-time Down cannot revise the consumed marker"
    );
    assert_eq!(down, original);

    for advance_first in [false, true] {
        let mut judge = judge(false, false, 0);
        judge
            .configure_hazards(timeline(vec![marker(1, 100, 1, 0)]))
            .unwrap();
        if advance_first {
            judge.advance_to(ts(100)).unwrap();
            assert_eq!(
                judge.hazard_events(),
                [event(1, 100, 1, 0, HazardOutcome::Avoided, None)]
            );
            judge.push_input(&down, ts(100)).unwrap();
        } else {
            judge.push_input(&down, ts(100)).unwrap();
            assert_eq!(
                judge.hazard_events(),
                [event(
                    1,
                    100,
                    1,
                    0,
                    HazardOutcome::Triggered,
                    Some(meta(u64::MAX))
                )]
            );
            judge.advance_to(ts(100)).unwrap();
        }
        assert!(judge.hazard_events().is_empty());
    }
}

#[test]
fn independent_buttons_and_enabled_contacts_hold_until_the_last_real_owner_releases() {
    let mut engine = judge(true, false, 0);
    engine
        .configure_hazards(timeline(
            (1..=13).map(|n| marker(n, n as i64, 1, n)).collect(),
        ))
        .unwrap();
    for input in [
        button(u64::MAX, u32::MAX, 1, ButtonState::Down),
        button(u64::MAX, u32::MAX, 1, ButtonState::Down), // Duplicate never adds an owner.
        button(u64::MAX - 1, u32::MAX, 1, ButtonState::Down),
        button(u64::MAX, u32::MAX - 1, 1, ButtonState::Down),
        touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Down),
        touch(u64::MAX, u32::MAX, 1, u64::MAX - 1, TouchPhase::Down),
        touch(1, u32::MAX, 1, u64::MAX, TouchPhase::Down),
        touch(1, u32::MAX, 1, u64::MAX, TouchPhase::Down),
        button(99, 99, 1, ButtonState::Repeat),
        button(98, 98, 1, ButtonState::Up),
    ] {
        assert!(engine.push_input(&input, ts(0)).unwrap().is_empty());
        assert!(engine.hazard_events().is_empty());
    }
    let operations = [
        (
            button(u64::MAX, u32::MAX, 1, ButtonState::Up),
            HazardOutcome::Triggered,
        ),
        (
            button(u64::MAX - 1, u32::MAX, 1, ButtonState::Up),
            HazardOutcome::Triggered,
        ),
        (
            button(u64::MAX, u32::MAX - 1, 1, ButtonState::Up),
            HazardOutcome::Triggered,
        ),
        (
            touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Cancel),
            HazardOutcome::Triggered,
        ),
        (
            touch(u64::MAX, u32::MAX, 1, u64::MAX - 1, TouchPhase::Up),
            HazardOutcome::Triggered,
        ),
        (
            touch(1, u32::MAX, 1, u64::MAX, TouchPhase::Move),
            HazardOutcome::Triggered,
        ),
        (
            touch(1, u32::MAX, 1, u64::MAX, TouchPhase::Cancel),
            HazardOutcome::Avoided,
        ),
        (
            button(99, 99, 1, ButtonState::Repeat),
            HazardOutcome::Avoided,
        ),
        (axis(1), HazardOutcome::Avoided),
        (touch(6, 6, 1, 6, TouchPhase::Move), HazardOutcome::Avoided),
        (
            button(u64::MAX, u32::MAX, 2, ButtonState::Down),
            HazardOutcome::Avoided,
        ),
        (
            touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Down),
            HazardOutcome::Triggered,
        ),
        (
            touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Cancel),
            HazardOutcome::Avoided,
        ),
    ];
    for (index, (input, outcome)) in operations.into_iter().enumerate() {
        let n = index as u64 + 1;
        assert!(engine.push_input(&input, ts(n as i64)).unwrap().is_empty());
        assert_eq!(
            engine.hazard_events(),
            [event(
                n,
                n as i64,
                1,
                n,
                outcome,
                Some(*input.physical.meta())
            )]
        );
    }
    assert!(engine.is_fresh_press(&button(u64::MAX, u32::MAX, 1, ButtonState::Down)));
    assert!(!engine.is_fresh_press(&button(u64::MAX, u32::MAX, 2, ButtonState::Down)));
    for contacts in [false, true] {
        let mut engine = judge(contacts, false, 0);
        engine
            .configure_hazards(timeline(vec![marker(1, 0, 1, 1)]))
            .unwrap();
        let input = touch(1, 1, 1, 1, TouchPhase::Down);
        engine.push_input(&input, ts(0)).unwrap();
        let outcome = if contacts {
            HazardOutcome::Triggered
        } else {
            HazardOutcome::Avoided
        };
        assert_eq!(
            engine.hazard_events(),
            [event(1, 0, 1, 1, outcome, Some(meta(1)))]
        );
    }
}

#[derive(Clone, Copy)]
struct InvalidSelection;
impl CandidateResolver for InvalidSelection {
    fn select(&self, _: &[Candidate]) -> Option<ObjectId> {
        Some(ObjectId(u64::MAX))
    }
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        Some(b"hazard-fixture-invalid-selection/v1".to_vec())
    }
}
struct UnsupportedPolicy;
impl JudgePolicy for UnsupportedPolicy {
    fn grade(&self, delta: i128, profile: &JudgeProfile) -> Option<JudgeGrade> {
        profile.grade(delta)
    }
}

#[test]
fn real_normal_judgments_are_unchanged_and_rejected_input_preserves_the_whole_hazard_state() {
    let mut engine = judge(false, true, 0);
    let mut legacy = judge(false, true, 0);
    engine
        .configure_hazards(timeline(vec![
            marker(1, 100, 1, 1),
            marker(2, 200, 1, 2),
            marker(3, 300, 1, 3),
            marker(4, 400, 1, 4),
        ]))
        .unwrap();
    for (at, state, stage, id, outcome) in [
        (
            100,
            ButtonState::Down,
            JudgeStage::Instant,
            1,
            HazardOutcome::Triggered,
        ),
        (
            200,
            ButtonState::Down,
            JudgeStage::HoldHead,
            2,
            HazardOutcome::Triggered,
        ),
        (
            300,
            ButtonState::Up,
            JudgeStage::HoldTail,
            2,
            HazardOutcome::Avoided,
        ),
    ] {
        if at == 200 {
            let up = button(1, 4, 1, ButtonState::Up);
            assert_eq!(
                engine.push_input(&up, ts(150)).unwrap(),
                legacy.push_input(&up, ts(150)).unwrap()
            );
        }
        let input = button(1, 4, 1, state);
        let actual = engine.push_input(&input, ts(at)).unwrap();
        assert_eq!(actual, legacy.push_input(&input, ts(at)).unwrap());
        assert_eq!(
            actual,
            [JudgeEvent {
                object: ObjectId(id),
                stage,
                at: ts(at),
                input: Some(meta(1)),
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(7),
                    delta: Duration::ZERO
                }
            }]
        );
        assert_eq!(
            engine.hazard_events(),
            [event(
                (at / 100) as u64,
                at,
                1,
                (at / 100) as u64,
                outcome,
                Some(meta(1))
            )]
        );
    }
    let misses = engine.advance_to(ts(401)).unwrap();
    assert_eq!(misses, legacy.advance_to(ts(401)).unwrap());
    assert_eq!(
        misses,
        [JudgeEvent {
            object: ObjectId(3),
            stage: JudgeStage::Instant,
            at: ts(401),
            input: None,
            outcome: JudgeOutcome::Miss {
                reason: MissReason::HeadTimeout
            }
        }]
    );
    assert_eq!(
        engine.hazard_events(),
        [event(4, 400, 1, 4, HazardOutcome::Avoided, None)]
    );
    assert!(legacy.hazard_events().is_empty());

    let mut rejected = JudgeEngine::with_policies(
        chart(true),
        rules(),
        profile(0),
        Box::new(InvalidSelection),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    rejected
        .configure_hazards(timeline(vec![
            marker(0, 0, 1, 0),
            marker(1, 50, 1, 1),
            marker(2, 100, 1, 2),
        ]))
        .unwrap();
    rejected.advance_to(ts(0)).unwrap();
    let before = rejected.stable_hash().unwrap();
    let report = rejected.hazard_events().to_vec();
    let down = button(1, 4, 1, ButtonState::Down);
    assert_eq!(
        rejected.push_input(&down, ts(100)),
        Err(JudgeError::InvalidCandidate {
            object: ObjectId(u64::MAX)
        })
    );
    assert_eq!(
        rejected.push_input(&down, ts(-1)),
        Err(JudgeError::NonMonotonicSongTime)
    );
    assert_eq!(
        rejected.advance_to(ts(-1)),
        Err(JudgeError::NonMonotonicSongTime)
    );
    assert_eq!(rejected.stable_hash().unwrap(), before);
    assert_eq!(rejected.hazard_events(), report);
    assert_eq!(rejected.effective_song_time(), Some(ts(0)));
    assert!(rejected.is_fresh_press(&down));
    assert_eq!(rejected.state(ObjectId(1)), Some(InteractionState::Pending));
    rejected.advance_to(ts(50)).unwrap();
    assert_eq!(
        rejected.hazard_events(),
        [event(1, 50, 1, 1, HazardOutcome::Avoided, None)]
    );
    rejected
        .push_input(&button(1, 4, 1, ButtonState::Up), ts(100))
        .unwrap();
    assert_eq!(
        rejected.hazard_events(),
        [event(2, 100, 1, 2, HazardOutcome::Avoided, Some(meta(1)))]
    );
    let mut retry = JudgeEngine::with_policies(
        chart(true),
        rules(),
        profile(0),
        Box::new(InvalidSelection),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    assert_eq!(
        retry.push_input(&down, ts(100)),
        Err(JudgeError::InvalidCandidate {
            object: ObjectId(u64::MAX)
        })
    );
    retry
        .configure_hazards(timeline(vec![marker(1, 100, 1, 1)]))
        .unwrap();
    retry.advance_to(ts(100)).unwrap();
    assert_eq!(
        retry.hazard_events(),
        [event(1, 100, 1, 1, HazardOutcome::Avoided, None)]
    );
}

#[test]
fn configuration_and_reusable_snapshots_keep_cursor_owners_reports_and_provenance_atomic() {
    let markers = vec![marker(1, 0, 1, 1), marker(2, 10, 1, 2), marker(3, 20, 1, 3)];
    let mut engine = judge(true, false, 0);
    engine.configure_hazards(timeline(markers.clone())).unwrap();
    let pristine = engine.snapshot().unwrap();
    let before = engine.stable_hash().unwrap();
    assert_eq!(
        engine.configure_hazards(timeline(markers.clone())),
        Err(HazardError::AlreadyConfigured)
    );
    assert_eq!(engine.stable_hash().unwrap(), before);
    let down = touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Down);
    engine.push_input(&down, ts(0)).unwrap();
    let checkpoint = engine.snapshot().unwrap();
    let held_hash = engine.stable_hash().unwrap();
    let held_report = engine.hazard_events().to_vec();
    let mut clone = JudgeEngine::from_snapshot(&checkpoint).unwrap();
    assert_eq!(clone.stable_hash().unwrap(), held_hash);
    assert_eq!(clone.hazard_events(), held_report);
    let mut compatible = judge(true, false, 0);
    compatible
        .configure_hazards(timeline(markers.iter().rev().copied().collect()))
        .unwrap();
    compatible.restore(&checkpoint).unwrap();
    assert_eq!(compatible.stable_hash().unwrap(), held_hash);
    for _ in 0..2 {
        clone.restore(&checkpoint).unwrap();
        clone
            .push_input(
                &touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Cancel),
                ts(10),
            )
            .unwrap();
        assert_eq!(
            clone.hazard_events(),
            [event(
                2,
                10,
                1,
                2,
                HazardOutcome::Avoided,
                Some(meta(u64::MAX))
            )]
        );
        clone.advance_to(ts(20)).unwrap();
        assert_eq!(
            clone.hazard_events(),
            [event(3, 20, 1, 3, HazardOutcome::Avoided, None)]
        );
    }
    assert_eq!(engine.stable_hash().unwrap(), held_hash);
    engine.advance_to(ts(20)).unwrap();
    assert_eq!(
        engine.hazard_events(),
        [
            event(2, 10, 1, 2, HazardOutcome::Triggered, None),
            event(3, 20, 1, 3, HazardOutcome::Triggered, None)
        ]
    );
    engine.restore(&checkpoint).unwrap();
    assert_eq!(engine.stable_hash().unwrap(), held_hash);
    assert_eq!(engine.hazard_events(), held_report);
    let report_hash = engine.stable_hash().unwrap();
    engine.advance_to(ts(0)).unwrap();
    assert!(engine.hazard_events().is_empty());
    assert_ne!(
        engine.stable_hash().unwrap(),
        report_hash,
        "the last report participates in complete state"
    );
    engine.restore(&pristine).unwrap();
    let mut other_meta = down.clone();
    if let PhysicalInputEvent::Touch(touch) = &mut other_meta.physical {
        touch.meta.sequence = 123;
    }
    engine.push_input(&other_meta, ts(0)).unwrap();
    assert_ne!(
        engine.stable_hash().unwrap(),
        held_hash,
        "identical ownership with different original provenance differs"
    );

    for changed in [
        vec![marker(99, 0, 1, 1), markers[1], markers[2]],
        vec![marker(1, -1, 1, 1), markers[1], markers[2]],
        vec![marker(1, 0, 2, 1), markers[1], markers[2]],
        vec![marker(1, 0, 1, u64::MAX), markers[1], markers[2]],
        vec![],
    ] {
        let mut incompatible = judge(true, false, 0);
        incompatible.configure_hazards(timeline(changed)).unwrap();
        let hash = incompatible.stable_hash().unwrap();
        assert_eq!(
            incompatible.restore(&checkpoint),
            Err(SnapshotError::ConfigurationMismatch)
        );
        assert_eq!(incompatible.stable_hash().unwrap(), hash);
    }
    let mut no_configuration = judge(true, false, 0);
    let hash = no_configuration.stable_hash().unwrap();
    assert_eq!(
        no_configuration.restore(&checkpoint),
        Err(SnapshotError::ConfigurationMismatch)
    );
    assert_eq!(no_configuration.stable_hash().unwrap(), hash);
    let mut empty = judge(false, false, 0);
    empty
        .configure_hazards(HazardTimeline::new(vec![], 0).unwrap())
        .unwrap();
    let empty_hash = empty.stable_hash().unwrap();
    assert_eq!(
        empty.configure_hazards(timeline(vec![])),
        Err(HazardError::AlreadyConfigured)
    );
    assert_eq!(empty.stable_hash().unwrap(), empty_hash);
    assert!(empty.advance_to(ts(20)).unwrap().is_empty());
    assert!(empty.hazard_events().is_empty());
    for input_first in [false, true] {
        let mut late = judge(false, false, 0);
        if input_first {
            late.push_input(&axis(9), ts(0)).unwrap();
        } else {
            late.advance_to(ts(0)).unwrap();
        }
        let prior = late.stable_hash().unwrap();
        assert_eq!(
            late.configure_hazards(timeline(markers.clone())),
            Err(HazardError::AlreadyStarted)
        );
        assert_eq!(late.stable_hash().unwrap(), prior);
        assert!(late.hazard_events().is_empty());
    }
    let mut unsupported = JudgeEngine::with_policies(
        chart(false),
        rules(),
        profile(0),
        Box::new(ClosestCandidate),
        Box::new(UnsupportedPolicy),
    )
    .unwrap();
    assert_eq!(
        unsupported.configure_hazards(timeline(markers)),
        Err(HazardError::Snapshot(SnapshotError::UnsupportedPolicy))
    );
    assert_eq!(unsupported.effective_song_time(), None);
    assert!(unsupported.hazard_events().is_empty());
    assert!(unsupported.advance_to(ts(20)).unwrap().is_empty());
    assert!(unsupported.hazard_events().is_empty());

    // Default construction and explicitly supplied legacy policies stay equal.
    let mut legacy = judge(false, true, 0);
    let mut policy = JudgeEngine::with_policies(
        chart(true),
        rules(),
        profile(0),
        Box::new(ClosestCandidate),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    assert_eq!(legacy.stable_hash().unwrap(), policy.stable_hash().unwrap());
    for (at, state) in [
        (100, ButtonState::Down),
        (150, ButtonState::Up),
        (200, ButtonState::Down),
        (300, ButtonState::Up),
    ] {
        let input = button(1, 4, 1, state);
        assert_eq!(
            legacy.push_input(&input, ts(at)).unwrap(),
            policy.push_input(&input, ts(at)).unwrap()
        );
        assert_eq!(legacy.stable_hash().unwrap(), policy.stable_hash().unwrap());
        assert!(legacy.hazard_events().is_empty() && policy.hazard_events().is_empty());
    }
}

#[test]
fn original_signed_marker_times_survive_long_sessions_offsets_and_checked_overflow() {
    let mut engine = judge(false, false, 0);
    engine
        .configure_hazards(timeline(vec![
            marker(0, i64::MIN, 1, 0),
            marker(1, 72_000_000_000_000, 1, 1),
            marker(2, 604_800_000_000_000, 1, 2),
            marker(u64::MAX, i64::MAX, 1, u64::MAX),
        ]))
        .unwrap();
    engine
        .push_input(&button(1, 4, 1, ButtonState::Down), ts(i64::MIN))
        .unwrap();
    assert_eq!(
        engine.hazard_events(),
        [event(
            0,
            i64::MIN,
            1,
            0,
            HazardOutcome::Triggered,
            Some(meta(1))
        )]
    );
    engine.advance_to(ts(72_000_000_000_000)).unwrap();
    assert_eq!(
        engine.hazard_events(),
        [event(
            1,
            72_000_000_000_000,
            1,
            1,
            HazardOutcome::Triggered,
            None
        )]
    );
    engine
        .push_input(&button(1, 4, 1, ButtonState::Up), ts(604_800_000_000_000))
        .unwrap();
    assert_eq!(
        engine.hazard_events(),
        [event(
            2,
            604_800_000_000_000,
            1,
            2,
            HazardOutcome::Avoided,
            Some(meta(1))
        )]
    );
    engine.advance_to(ts(i64::MAX)).unwrap();
    assert_eq!(
        engine.hazard_events(),
        [event(
            u64::MAX,
            i64::MAX,
            1,
            u64::MAX,
            HazardOutcome::Avoided,
            None
        )]
    );
    let hash = engine.stable_hash().unwrap();
    assert_eq!(
        engine.advance_to(ts(i64::MAX - 1)),
        Err(JudgeError::NonMonotonicSongTime)
    );
    assert_eq!(engine.stable_hash().unwrap(), hash);

    for (offset, mapped, boundary) in [(7, 93, 100), (-7, 107, 100)] {
        let mut input = judge(false, false, offset);
        let mut advance = judge(false, false, offset);
        input
            .configure_hazards(timeline(vec![marker(1, boundary, 1, 1)]))
            .unwrap();
        advance
            .configure_hazards(timeline(vec![marker(1, boundary, 1, 1)]))
            .unwrap();
        input
            .push_input(&button(1, 4, 1, ButtonState::Down), ts(mapped))
            .unwrap();
        advance.advance_to(ts(mapped)).unwrap();
        assert_eq!(input.effective_song_time(), Some(ts(100)));
        assert_eq!(advance.effective_song_time(), Some(ts(100)));
        assert_eq!(
            input.hazard_events(),
            [event(1, 100, 1, 1, HazardOutcome::Triggered, Some(meta(1)))]
        );
        assert_eq!(
            advance.hazard_events(),
            [event(1, 100, 1, 1, HazardOutcome::Avoided, None)]
        );
    }
    for (offset, invalid, mapped, boundary) in [
        (1, i64::MAX, i64::MAX - 1, i64::MAX),
        (-1, i64::MIN, i64::MIN + 1, i64::MIN),
    ] {
        let mut engine = judge(false, false, offset);
        let down = button(1, 4, 1, ButtonState::Down);
        let initial = engine.stable_hash().unwrap();
        assert_eq!(
            engine.push_input(&down, ts(invalid)),
            Err(JudgeError::Overflow)
        );
        assert_eq!(engine.advance_to(ts(invalid)), Err(JudgeError::Overflow));
        assert_eq!(engine.stable_hash().unwrap(), initial);
        engine
            .configure_hazards(timeline(vec![marker(1, boundary, 1, 1)]))
            .unwrap();
        engine.push_input(&down, ts(mapped)).unwrap();
        let report = vec![event(
            1,
            boundary,
            1,
            1,
            HazardOutcome::Triggered,
            Some(meta(1)),
        )];
        assert_eq!(engine.hazard_events(), report);
        let accepted = engine.stable_hash().unwrap();
        assert_eq!(engine.advance_to(ts(invalid)), Err(JudgeError::Overflow));
        assert_eq!(engine.hazard_events(), report);
        assert_eq!(engine.stable_hash().unwrap(), accepted);
    }
}
