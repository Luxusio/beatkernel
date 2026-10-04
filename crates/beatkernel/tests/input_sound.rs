//! Deferred immutable input-sound policy and actual JudgeEngine ownership fixtures.
use beatkernel::{
    audio::{AudioCommand, SampleId, VoiceId},
    chart::*,
    input::*,
    interaction::{HoldEvaluator, InstantEvaluator, PressHoldEvaluator, PressInstantEvaluator},
    judge::*,
    runtime::input_sound::{InputSoundError, InputSoundMarker, InputSoundTimeline},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn marker(control: u32, at: i64, sample: u64, voice: u64, gain: f32) -> InputSoundMarker {
    InputSoundMarker {
        control: GameControlId(control),
        at: ts(at),
        sample: SampleId(sample),
        voice: VoiceId(voice),
        gain,
    }
}
fn play(sample: u64, voice: u64, at: i64, gain: f32) -> AudioCommand {
    AudioCommand::Play {
        sample: SampleId(sample),
        voice: VoiceId(voice),
        at: ts(at),
        gain,
    }
}
fn meta(source: u64) -> EventMeta {
    EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: ts(-99),
        },
        u64::MAX,
    )
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
            position: Position2 { x: -2.5, y: 19.25 },
            pressure: Some(0.75),
        }),
    }
}
fn judge(contacts: bool) -> JudgeEngine {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = [(1, 100, None, 1), (2, 200, Some(300), 2)]
        .into_iter()
        .map(|(id, start, end, kind)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(start).unwrap(),
            end: end.map(|end| Beat::new(end).unwrap()),
            interaction: InteractionId(kind),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    JudgeEngine::new(
        source.compile().unwrap(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: if contacts {
                    Box::new(PressInstantEvaluator)
                } else {
                    Box::new(InstantEvaluator)
                },
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(1),
                evaluator: if contacts {
                    Box::new(PressHoldEvaluator)
                } else {
                    Box::new(HoldEvaluator)
                },
            },
        ],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(9),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn owned_index_validates_capacity_and_positions_and_selects_literal_signed_full_width_boundaries() {
    let timeline = InputSoundTimeline::new(
        vec![
            marker(u32::MAX, i64::MAX, u64::MAX, u64::MAX, -2.5),
            marker(0, i64::MIN, 0, 0, f32::MAX),
            marker(7, 604_800_000_000_000, 3, 9, 0.0),
            marker(7, -1, 1, 9, -0.0),
            marker(7, 72_000_000_000_000, 2, 9, 0.5),
        ],
        5,
    )
    .unwrap();
    let before = timeline.clone();
    for (song, expected) in [
        (i64::MIN, None),
        (-2, None),
        (-1, Some(play(1, 9, -777, -0.0))),
        (0, Some(play(1, 9, -777, -0.0))),
        (71_999_999_999_999, Some(play(1, 9, -777, -0.0))),
        (72_000_000_000_000, Some(play(2, 9, -777, 0.5))),
        (604_800_000_000_000, Some(play(3, 9, -777, 0.0))),
        (i64::MAX, Some(play(3, 9, -777, 0.0))),
    ] {
        assert_eq!(
            timeline.command_for(GameControlId(7), ts(song), ts(-777)),
            expected
        );
    }
    assert_eq!(
        timeline.command_for(GameControlId(0), ts(i64::MIN), ts(i64::MAX)),
        Some(play(0, 0, i64::MAX, f32::MAX))
    );
    assert_eq!(
        timeline.command_for(GameControlId(u32::MAX), ts(i64::MAX - 1), ts(0)),
        None
    );
    assert_eq!(
        timeline.command_for(GameControlId(u32::MAX), ts(i64::MAX), ts(i64::MIN)),
        Some(play(u64::MAX, u64::MAX, i64::MIN, -2.5))
    );
    assert_eq!(
        timeline.command_for(GameControlId(6), ts(i64::MAX), ts(0)),
        None
    );
    if let Some(AudioCommand::Play { gain, .. }) =
        timeline.command_for(GameControlId(7), ts(0), ts(1))
    {
        assert_eq!(gain.to_bits(), (-0.0f32).to_bits());
    } else {
        panic!("the admitted marker must preserve its signed zero gain");
    }
    assert_eq!(timeline, before, "queries do not advance a retained cursor");
    assert!(
        InputSoundTimeline::new(vec![], 1)
            .unwrap()
            .command_for(GameControlId(0), ts(0), ts(0))
            .is_none()
    );
    for cap in [0, 100_001, usize::MAX] {
        assert_eq!(
            InputSoundTimeline::new(vec![], cap).unwrap_err(),
            InputSoundError::InvalidCapacity
        );
    }
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            InputSoundTimeline::new(vec![marker(1, 0, 1, 1, gain)], 1).unwrap_err(),
            InputSoundError::InvalidGain
        );
    }
    assert_eq!(
        InputSoundTimeline::new(vec![marker(1, 0, 1, 1, 1.0), marker(1, 0, 2, 2, 0.5)], 2)
            .unwrap_err(),
        InputSoundError::DuplicatePosition {
            control: GameControlId(1),
            at: ts(0)
        }
    );
    let dense: Vec<_> = (0..100_000)
        .rev()
        .map(|index| marker(12, index - 50_000, (index + 1) as u64, 4, 1.0))
        .collect();
    let bounded = InputSoundTimeline::new(dense, 100_000).unwrap();
    assert_eq!(
        bounded.command_for(GameControlId(12), ts(-50_001), ts(3)),
        None
    );
    assert_eq!(
        bounded.command_for(GameControlId(12), ts(-50_000), ts(3)),
        Some(play(1, 4, 3, 1.0))
    );
    assert_eq!(
        bounded.command_for(GameControlId(12), ts(49_999), ts(4)),
        Some(play(100_000, 4, 4, 1.0))
    );
    assert_eq!(
        InputSoundTimeline::new(vec![marker(1, 0, 1, 1, 1.0); 100_001], 100_000).unwrap_err(),
        InputSoundError::TooManyMarkers
    );
}

#[test]
fn readonly_freshness_uses_real_button_and_enabled_contact_ownership_and_restored_checkpoints() {
    let mut engine = judge(true);
    let key = button(u64::MAX, u32::MAX, 1, ButtonState::Down);
    let initial = engine.stable_hash().unwrap();
    assert!(engine.is_fresh_press(&key));
    assert!(engine.is_fresh_press(&key));
    assert_eq!(engine.stable_hash().unwrap(), initial);
    assert!(engine.push_input(&key, ts(0)).unwrap().is_empty());
    assert!(!engine.is_fresh_press(&key));
    for other in [
        button(u64::MAX - 1, u32::MAX, 1, ButtonState::Down),
        button(u64::MAX, 0, 1, ButtonState::Down),
        button(u64::MAX, u32::MAX, 2, ButtonState::Down),
    ] {
        assert!(engine.is_fresh_press(&other));
    }
    engine
        .push_input(&button(5, u32::MAX, 1, ButtonState::Up), ts(0))
        .unwrap();
    assert!(!engine.is_fresh_press(&key));
    for state in [ButtonState::Up, ButtonState::Repeat] {
        assert!(!engine.is_fresh_press(&button(u64::MAX, u32::MAX, 1, state)));
    }
    engine
        .push_input(&button(u64::MAX, u32::MAX, 1, ButtonState::Up), ts(0))
        .unwrap();
    assert!(engine.is_fresh_press(&key));
    let contact = touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Down);
    assert!(engine.is_fresh_press(&contact));
    engine.push_input(&contact, ts(0)).unwrap();
    assert!(!engine.is_fresh_press(&contact));
    assert!(
        engine.is_fresh_press(&key),
        "contact ownership does not alias button ownership"
    );
    for other in [
        touch(u64::MAX, u32::MAX, 1, u64::MAX - 1, TouchPhase::Down),
        touch(u64::MAX - 1, u32::MAX, 1, u64::MAX, TouchPhase::Down),
        touch(u64::MAX, 0, 1, u64::MAX, TouchPhase::Down),
        touch(u64::MAX, u32::MAX, 2, u64::MAX, TouchPhase::Down),
    ] {
        assert!(engine.is_fresh_press(&other));
    }
    let checkpoint = engine.snapshot().unwrap();
    engine
        .push_input(&touch(u64::MAX, u32::MAX, 1, 7, TouchPhase::Cancel), ts(0))
        .unwrap();
    assert!(!engine.is_fresh_press(&contact));
    engine
        .push_input(
            &touch(u64::MAX, u32::MAX, 1, u64::MAX, TouchPhase::Cancel),
            ts(0),
        )
        .unwrap();
    assert!(engine.is_fresh_press(&contact));
    engine.restore(&checkpoint).unwrap();
    assert!(!engine.is_fresh_press(&contact));
    let restored = JudgeEngine::from_snapshot(&checkpoint).unwrap();
    assert!(!restored.is_fresh_press(&contact));
    let mut legacy = judge(false);
    assert!(!legacy.is_fresh_press(&contact));
    legacy.push_input(&contact, ts(0)).unwrap();
    assert!(!legacy.is_fresh_press(&contact));
    assert!(legacy.is_fresh_press(&key));
}

#[test]
fn pure_press_policy_uses_actual_head_hits_and_never_turns_release_repeat_motion_or_misses_into_head_priority()
 {
    let timeline = InputSoundTimeline::new(vec![marker(1, 0, 8, 9, 0.5)], 1).unwrap();
    let mut engine = judge(true);
    let down = button(1, 1, 1, ButtonState::Down);
    let fresh = engine.is_fresh_press(&down);
    let hit = engine.push_input(&down, ts(100)).unwrap();
    assert_eq!(hit[0].stage, JudgeStage::Instant);
    assert!(matches!(hit[0].outcome, JudgeOutcome::Hit { .. }));
    assert_eq!(
        timeline.command_for_press(&down, fresh, ts(100), ts(900), &hit),
        None
    );
    engine
        .push_input(&button(1, 1, 1, ButtonState::Up), ts(101))
        .unwrap();
    let contact = touch(1, 1, 1, 4, TouchPhase::Down);
    let fresh = engine.is_fresh_press(&contact);
    let head = engine.push_input(&contact, ts(200)).unwrap();
    assert_eq!(head[0].stage, JudgeStage::HoldHead);
    assert_eq!(
        timeline.command_for_press(&contact, fresh, ts(200), ts(901), &head),
        None
    );
    let up = touch(1, 1, 1, 4, TouchPhase::Up);
    let tail = engine.push_input(&up, ts(300)).unwrap();
    assert_eq!(tail[0].stage, JudgeStage::HoldTail);
    assert!(matches!(tail[0].outcome, JudgeOutcome::Hit { .. }));
    assert_eq!(
        timeline.command_for_press(&up, true, ts(300), ts(902), &tail),
        None
    );
    assert_eq!(
        timeline.command_for_press(&down, true, ts(301), ts(903), &tail),
        Some(play(8, 9, 903, 0.5))
    );
    let misses = judge(true).advance_to(ts(400)).unwrap();
    assert!(
        misses
            .iter()
            .all(|event| matches!(event.outcome, JudgeOutcome::Miss { .. }))
    );
    assert_eq!(
        timeline.command_for_press(&down, true, ts(401), ts(904), &misses),
        Some(play(8, 9, 904, 0.5))
    );
    assert_eq!(
        timeline.command_for_press(&down, false, ts(401), ts(904), &[]),
        None
    );
    let mut ignored = vec![
        button(1, 1, 1, ButtonState::Up),
        button(1, 1, 1, ButtonState::Repeat),
    ];
    ignored.extend(
        [TouchPhase::Move, TouchPhase::Up, TouchPhase::Cancel]
            .map(|phase| touch(1, 1, 1, 4, phase)),
    );
    for physical_event in [
        PhysicalInputEvent::Axis(AxisEvent {
            meta: meta(1),
            control: physical(1),
            value: 1.0,
            mode: AxisMode::Absolute,
        }),
        PhysicalInputEvent::Pointer(PointerEvent {
            meta: meta(1),
            control: physical(1),
            position: Position2 { x: 1.0, y: 2.0 },
            mode: PointerMode::Relative,
        }),
        PhysicalInputEvent::Custom(CustomInputEvent {
            meta: meta(1),
            namespace: VendorNamespaceId(7),
            type_id: 1,
            payload: vec![1],
        }),
    ] {
        ignored.push(GameInputEvent {
            game_control: GameControlId(1),
            physical: physical_event,
        });
    }
    for input in ignored {
        assert!(!engine.is_fresh_press(&input));
        assert_eq!(
            timeline.command_for_press(&input, true, ts(401), ts(904), &[]),
            None
        );
    }
}
