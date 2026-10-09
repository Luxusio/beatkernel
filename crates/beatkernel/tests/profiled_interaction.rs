use beatkernel::{
    chart::*,
    input::*,
    interaction::*,
    judge::*,
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn profile(early: i64, late: i64, offset: i64) -> JudgeProfile {
    JudgeProfile::new(
        vec![
            JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::from_nanos(early / 2),
                late: Duration::from_nanos(late / 2),
            },
            JudgeWindow {
                grade: JudgeGrade(9),
                early: Duration::from_nanos(early),
                late: Duration::from_nanos(late),
            },
        ],
        Duration::from_nanos(offset),
    )
    .unwrap()
}
fn chart(hold: bool) -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects.push(SourceObject {
        id: ObjectId(1),
        start: Beat::new(100).unwrap(),
        end: hold.then(|| Beat::new(200).unwrap()),
        interaction: InteractionId(1),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    compile(&source).unwrap()
}
fn build(
    hold: bool,
    head: JudgeProfile,
    tail: JudgeProfile,
    contact: bool,
    envelope: JudgeProfile,
) -> Result<JudgeEngine, JudgeError> {
    let evaluator: Box<dyn InteractionEvaluator> = if hold {
        Box::new(ProfiledHoldEvaluator::new(head, tail, contact)?)
    } else {
        Box::new(ProfiledInstantEvaluator::new(head, contact)?)
    };
    JudgeEngine::new(
        chart(hold),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator,
        }],
        envelope,
    )
}
fn engine(hold: bool, contact: bool, offset: i64) -> JudgeEngine {
    build(
        hold,
        profile(10, 20, 0),
        profile(30, 40, 0),
        contact,
        profile(50, 60, offset),
    )
    .unwrap()
}
fn meta(source: u64) -> EventMeta {
    EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(1),
            timestamp: ts(0),
        },
        0,
    )
}
fn button(source: u64, state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: meta(source),
            control: PhysicalControlId::keyboard(4),
            state,
        }),
    }
}
fn contact(source: u64, id: u64, phase: TouchPhase) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: meta(source),
            control: PhysicalControlId::keyboard(4),
            contact: ContactId(id),
            phase,
            position: Position2 { x: 0.0, y: 0.0 },
            pressure: None,
        }),
    }
}
fn hit(delta: i64, grade: u32) -> JudgeOutcome {
    JudgeOutcome::Hit {
        grade: JudgeGrade(grade),
        delta: Duration::from_nanos(delta),
    }
}

#[test]
fn selected_head_grades_and_accepts_inclusive_asymmetric_bounds() {
    for (delta, expected) in [
        (-11, None),
        (-10, Some(9)),
        (-6, Some(9)),
        (-5, Some(7)),
        (0, Some(7)),
        (10, Some(7)),
        (11, Some(9)),
        (20, Some(9)),
    ] {
        let mut engine = engine(false, false, 0);
        let result = engine
            .push_input(&button(1, ButtonState::Down), ts(100 + delta))
            .unwrap();
        if let Some(grade) = expected {
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].outcome, hit(delta, grade));
        } else {
            assert!(result.is_empty());
            assert_eq!(engine.state(ObjectId(1)), Some(InteractionState::Pending));
        }
    }
    let mut engine = engine(false, false, 0);
    let results = engine
        .push_input(&button(1, ButtonState::Down), ts(121))
        .unwrap();
    assert_eq!(
        results[0].outcome,
        JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout
        }
    );
}

#[test]
fn selected_tail_grades_and_timeouts_can_extend_beyond_head_envelope() {
    for (delta, expected) in [
        (
            -31,
            JudgeOutcome::Miss {
                reason: MissReason::EarlyRelease,
            },
        ),
        (-30, hit(-30, 9)),
        (-16, hit(-16, 9)),
        (-15, hit(-15, 7)),
        (20, hit(20, 7)),
        (21, hit(21, 9)),
        (40, hit(40, 9)),
        (
            41,
            JudgeOutcome::Miss {
                reason: MissReason::TailTimeout,
            },
        ),
    ] {
        let mut engine = build(
            true,
            profile(10, 20, 0),
            profile(30, 40, 0),
            false,
            profile(10, 20, 0),
        )
        .unwrap();
        assert_eq!(
            engine
                .push_input(&button(1, ButtonState::Down), ts(100))
                .unwrap()[0]
                .outcome,
            hit(0, 7)
        );
        let result = engine
            .push_input(&button(1, ButtonState::Up), ts(200 + delta))
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].stage, JudgeStage::HoldTail);
        assert_eq!(result[0].outcome, expected);
    }
}

#[test]
fn actual_stage_deadlines_expire_strictly_after_selected_late_bound() {
    let mut pending = engine(true, false, 0);
    assert!(pending.advance_to(ts(120)).unwrap().is_empty());
    assert_eq!(
        pending.advance_to(ts(121)).unwrap()[0].outcome,
        JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout
        }
    );
    let mut active = engine(true, false, 0);
    active
        .push_input(&button(1, ButtonState::Down), ts(100))
        .unwrap();
    assert!(active.advance_to(ts(240)).unwrap().is_empty());
    assert_eq!(
        active.advance_to(ts(241)).unwrap()[0].outcome,
        JudgeOutcome::Miss {
            reason: MissReason::TailTimeout
        }
    );
    assert!(active.advance_to(ts(500)).unwrap().is_empty());
}

#[test]
fn offset_is_applied_once_to_heads_tails_and_deadlines() {
    for offset in [-15, 15] {
        let mut engine = engine(true, false, offset);
        assert_eq!(
            engine
                .push_input(&button(1, ButtonState::Down), ts(100 - offset))
                .unwrap()[0]
                .outcome,
            hit(0, 7)
        );
        assert!(engine.advance_to(ts(240 - offset)).unwrap().is_empty());
        assert_eq!(
            engine
                .push_input(&button(1, ButtonState::Up), ts(240 - offset))
                .unwrap()[0]
                .outcome,
            hit(40, 9)
        );
        let mut pending = self::engine(false, false, offset);
        assert!(pending.advance_to(ts(120 - offset)).unwrap().is_empty());
        assert_eq!(
            pending.advance_to(ts(121 - offset)).unwrap()[0].outcome,
            JudgeOutcome::Miss {
                reason: MissReason::HeadTimeout
            }
        );
    }
}

#[test]
fn contact_ownership_cancel_repeat_and_button_only_selection_are_preserved() {
    let mut engine = engine(true, true, 0);
    assert_eq!(
        engine
            .push_input(&contact(1, 7, TouchPhase::Down), ts(100))
            .unwrap()[0]
            .outcome,
        hit(0, 7)
    );
    for event in [
        contact(2, 7, TouchPhase::Up),
        contact(1, 8, TouchPhase::Cancel),
        button(1, ButtonState::Up),
    ] {
        assert!(engine.push_input(&event, ts(190)).unwrap().is_empty());
    }
    assert_eq!(
        engine
            .push_input(&contact(1, 7, TouchPhase::Cancel), ts(200))
            .unwrap()[0]
            .outcome,
        JudgeOutcome::Miss {
            reason: MissReason::RejectedInput
        }
    );
    let mut buttons = self::engine(true, false, 0);
    assert!(buttons
        .push_input(&contact(1, 7, TouchPhase::Down), ts(100))
        .unwrap()
        .is_empty());
    assert!(buttons
        .push_input(&button(1, ButtonState::Repeat), ts(100))
        .unwrap()
        .is_empty());
    buttons
        .push_input(&button(1, ButtonState::Down), ts(100))
        .unwrap();
    assert!(buttons
        .push_input(&button(2, ButtonState::Up), ts(200))
        .unwrap()
        .is_empty());
    assert_eq!(
        buttons
            .push_input(&button(1, ButtonState::Up), ts(200))
            .unwrap()[0]
            .outcome,
        hit(0, 7)
    );
}

#[test]
fn invalid_offsets_head_envelopes_and_grade_identity_are_refused() {
    assert!(matches!(
        ProfiledInstantEvaluator::new(profile(10, 20, 1), false),
        Err(JudgeError::InvalidProfile)
    ));
    assert!(matches!(
        ProfiledHoldEvaluator::new(profile(10, 20, 0), profile(30, 40, -1), false),
        Err(JudgeError::InvalidProfile)
    ));
    assert!(matches!(
        build(
            false,
            profile(11, 20, 0),
            profile(30, 40, 0),
            false,
            profile(10, 20, 0)
        ),
        Err(JudgeError::InvalidProfile)
    ));
    assert!(matches!(
        build(
            false,
            profile(10, 21, 0),
            profile(30, 40, 0),
            false,
            profile(10, 20, 0)
        ),
        Err(JudgeError::InvalidProfile)
    ));
    let different = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(8),
            early: Duration::from_nanos(30),
            late: Duration::from_nanos(40),
        }],
        Duration::ZERO,
    )
    .unwrap();
    assert!(matches!(
        build(
            true,
            profile(10, 20, 0),
            different,
            false,
            profile(50, 60, 0)
        ),
        Err(JudgeError::InvalidProfile)
    ));
}

#[test]
fn full_profile_identity_discriminates_same_envelope_and_restores_owned_state() {
    let mut first = engine(true, true, 0);
    let origin = first.snapshot().unwrap();
    let mut second = build(
        true,
        profile(10, 20, 0),
        profile(30, 42, 0),
        true,
        profile(50, 60, 0),
    )
    .unwrap();
    assert_ne!(first.stable_hash().unwrap(), second.stable_hash().unwrap());
    assert_eq!(
        second.restore(&origin),
        Err(SnapshotError::ConfigurationMismatch)
    );
    // Same widest bounds, different inner windows must also refuse restoration.
    let different = JudgeProfile::new(
        vec![
            JudgeWindow {
                grade: JudgeGrade(7),
                early: Duration::from_nanos(1),
                late: Duration::from_nanos(2),
            },
            JudgeWindow {
                grade: JudgeGrade(9),
                early: Duration::from_nanos(10),
                late: Duration::from_nanos(20),
            },
        ],
        Duration::ZERO,
    )
    .unwrap();
    let mut third = build(
        true,
        different,
        profile(30, 40, 0),
        true,
        profile(50, 60, 0),
    )
    .unwrap();
    assert_ne!(first.stable_hash().unwrap(), third.stable_hash().unwrap());
    assert_eq!(
        third.restore(&origin),
        Err(SnapshotError::ConfigurationMismatch)
    );
    first
        .push_input(&contact(1, 7, TouchPhase::Down), ts(100))
        .unwrap();
    let active = first.snapshot().unwrap();
    let before = first.stable_hash().unwrap();
    let expected = first
        .push_input(&contact(1, 7, TouchPhase::Up), ts(225))
        .unwrap();
    let after = first.stable_hash().unwrap();
    for _ in 0..2 {
        first.restore(&active).unwrap();
        assert_eq!(first.stable_hash().unwrap(), before);
        assert!(first
            .push_input(&contact(1, 8, TouchPhase::Up), ts(200))
            .unwrap()
            .is_empty());
        assert_eq!(
            first
                .push_input(&contact(1, 7, TouchPhase::Up), ts(225))
                .unwrap(),
            expected
        );
        assert_eq!(first.stable_hash().unwrap(), after);
    }
}

#[test]
fn profiled_rules_retain_builtin_index_routing() {
    assert_eq!(
        ProfiledInstantEvaluator::new(profile(10, 20, 0), false)
            .unwrap()
            .start_eligibility(),
        StartEligibility::ProfileButtonPress
    );
    assert_eq!(
        ProfiledHoldEvaluator::new(profile(10, 20, 0), profile(30, 40, 0), true)
            .unwrap()
            .start_eligibility(),
        StartEligibility::ProfilePress
    );
}

#[test]
fn indexed_candidates_filter_selected_heads_and_each_owner_uses_selected_tail() {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    for id in 1..=2 {
        source.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(100).unwrap(),
            end: Some(Beat::new(200).unwrap()),
            interaction: InteractionId(id as u32),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let mut engine = JudgeEngine::new(
        compile(&source).unwrap(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(
                    ProfiledHoldEvaluator::new(profile(10, 20, 0), profile(10, 20, 0), false)
                        .unwrap(),
                ),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(1),
                evaluator: Box::new(
                    ProfiledHoldEvaluator::new(profile(20, 20, 0), profile(30, 40, 0), false)
                        .unwrap(),
                ),
            },
        ],
        profile(20, 20, 0),
    )
    .unwrap();
    let result = engine
        .push_input(&button(2, ButtonState::Down), ts(85))
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].object, ObjectId(2));
    assert_eq!(result[0].outcome, hit(-15, 9));
    let result = engine
        .push_input(&button(1, ButtonState::Down), ts(100))
        .unwrap();
    assert_eq!(result[0].object, ObjectId(1));
    let result = engine
        .push_input(&button(1, ButtonState::Up), ts(219))
        .unwrap();
    assert_eq!(result[0].object, ObjectId(1));
    assert_eq!(result[0].outcome, hit(19, 9));
    let result = engine
        .push_input(&button(2, ButtonState::Up), ts(220))
        .unwrap();
    assert_eq!(result[0].object, ObjectId(2));
    assert_eq!(result[0].outcome, hit(20, 7));
}

#[test]
fn original_builtin_pending_snapshot_bytes_keep_their_exact_schema_and_layout() {
    for (hold, contacts) in [(false, false), (true, false), (false, true), (true, true)] {
        let chart = chart(hold);
        let envelope = profile(50, 60, 0);
        let evaluator: Box<dyn InteractionEvaluator> = match (hold, contacts) {
            (false, false) => Box::new(InstantEvaluator),
            (true, false) => Box::new(HoldEvaluator),
            (false, true) => Box::new(PressInstantEvaluator),
            (true, true) => Box::new(PressHoldEvaluator),
        };
        let interaction = evaluator.begin(
            &chart.objects()[0],
            &BeginContext {
                control: GameControlId(1),
                profile: &envelope,
            },
        );
        let schema: &[u8] = if contacts {
            b"press-interaction/v1"
        } else {
            b"button-interaction/v1"
        };
        let mut expected = (schema.len() as u64).to_le_bytes().to_vec();
        expected.extend_from_slice(schema);
        expected.extend_from_slice(&100i64.to_le_bytes());
        expected.push(u8::from(hold));
        if hold {
            expected.extend_from_slice(&200i64.to_le_bytes());
        }
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.push(0); // Pending.
        expected.push(0); // No owner.
        assert_eq!(interaction.snapshot_bytes().unwrap(), expected);
    }
}

#[test]
fn caller_grading_policy_receives_actual_stage_profile_without_offset() {
    struct InspectPolicy;
    impl JudgePolicy for InspectPolicy {
        fn grade(&self, delta: i128, selected: &JudgeProfile) -> Option<JudgeGrade> {
            assert_eq!(selected.input_offset(), Duration::ZERO);
            // Distinguish the selected head, selected tail, and global envelope.
            match selected.max_late().as_nanos() {
                20 => Some(JudgeGrade(7)),
                40 => Some(JudgeGrade(9)),
                _ => panic!("global routing envelope used for stage grading"),
            }
            .filter(|_| selected.grade(delta).is_some())
        }
    }
    let mut engine = JudgeEngine::with_policies(
        chart(true),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(
                ProfiledHoldEvaluator::new(profile(10, 20, 0), profile(30, 40, 0), false).unwrap(),
            ),
        }],
        profile(50, 60, 15),
        Box::new(ClosestCandidate),
        Box::new(InspectPolicy),
    )
    .unwrap();
    assert_eq!(
        engine
            .push_input(&button(1, ButtonState::Down), ts(85))
            .unwrap()[0]
            .outcome,
        hit(0, 7)
    );
    assert_eq!(
        engine
            .push_input(&button(1, ButtonState::Up), ts(185))
            .unwrap()[0]
            .outcome,
        hit(0, 9)
    );
}
