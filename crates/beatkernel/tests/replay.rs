use beatkernel::{chart::*, input::*, interaction::*, judge::*, replay::*, time::*};
fn ts(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}
fn chart() -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = [(1, 100, Some(200), 1), (2, 300, None, 2)]
        .into_iter()
        .map(|(id, start, end, interaction)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(start).unwrap(),
            end: end.map(|end| Beat::new(end).unwrap()),
            interaction: InteractionId(interaction),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    compile(&source).unwrap()
}
fn engine() -> JudgeEngine {
    JudgeEngine::new(
        chart(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(HoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(1),
                evaluator: Box::new(InstantEvaluator),
            },
        ],
        profile(),
    )
    .unwrap()
}
fn profile() -> JudgeProfile {
    JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(10),
            late: Duration::from_nanos(10),
        }],
        Duration::ZERO,
    )
    .unwrap()
}
fn button(source: u64, state: ButtonState, sequence: u64) -> GameInputEvent {
    let mut meta = EventMeta::new(
        DeviceId(source),
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: ts(1000),
        },
        sequence,
    );
    meta.native = Some(NativeEventMeta {
        backend: BackendId(2),
        code: Some(42),
        timestamp: Some(ClockPoint {
            domain: ClockDomainId(8),
            timestamp: ts(999),
        }),
    });
    GameInputEvent {
        game_control: GameControlId(1),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta,
            control: PhysicalControlId::keyboard(4),
            state,
        }),
    }
}
fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"chart/1".to_vec(),
        rules_identity: b"builtin/1".to_vec(),
        options: vec![1, 2],
        seed: 73,
        normalized_clock: ClockDomainId(7),
    }
}
#[test]
fn reusable_snapshot_preserves_hold_owner_and_results() {
    let mut judge = engine();
    judge
        .push_input(&button(1, ButtonState::Down, 1), ts(100))
        .unwrap();
    let snapshot = judge.snapshot().unwrap();
    let original_hash = judge.stable_hash().unwrap();
    assert!(judge
        .push_input(&button(2, ButtonState::Up, 2), ts(200))
        .unwrap()
        .is_empty());
    let first = judge
        .push_input(&button(1, ButtonState::Up, 3), ts(200))
        .unwrap();
    assert_eq!(first[0].stage, JudgeStage::HoldTail);
    judge.restore(&snapshot).unwrap();
    assert_eq!(judge.stable_hash().unwrap(), original_hash);
    assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Active));
    assert!(judge.is_held(InputOwner {
        source: DeviceId(1),
        physical: PhysicalControlId::keyboard(4),
        game_control: GameControlId(1)
    }));
    assert_eq!(
        judge
            .push_input(&button(1, ButtonState::Up, 3), ts(200))
            .unwrap(),
        first
    );
    judge.restore(&snapshot).unwrap();
    assert_eq!(judge.stable_hash().unwrap(), original_hash);
}
#[test]
fn hashes_include_actual_owner_configuration_and_policy() {
    let mut one = engine();
    let mut two = engine();
    one.push_input(&button(1, ButtonState::Down, 1), ts(100))
        .unwrap();
    two.push_input(&button(2, ButtonState::Down, 1), ts(100))
        .unwrap();
    assert_ne!(one.stable_hash().unwrap(), two.stable_hash().unwrap());
    let mut differing = JudgeEngine::with_policies(
        chart(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(HoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(1),
                evaluator: Box::new(InstantEvaluator),
            },
        ],
        profile(),
        Box::new(EarliestCandidate),
        Box::new(WindowJudgePolicy),
    )
    .unwrap();
    differing
        .push_input(&button(1, ButtonState::Down, 1), ts(100))
        .unwrap();
    assert_ne!(one.stable_hash().unwrap(), differing.stable_hash().unwrap());
}
#[test]
fn replay_preserves_recorded_advances_and_reconstructs_boundary_misses() {
    let mut replay = ReplaySession::new(header(), engine()).unwrap();
    replay
        .push_input(button(1, ButtonState::Down, 1), ts(100))
        .unwrap();
    replay.checkpoint().unwrap();
    replay.advance_to(ts(500)).unwrap();
    replay.checkpoint().unwrap();
    assert_eq!(replay.results()[1].at, ts(500));
    let terminal_hash = replay.stable_hash().unwrap();
    replay.seek(ts(250)).unwrap();
    assert_eq!(replay.results()[1].at, ts(250));
    assert_eq!(replay.engine().effective_song_time(), Some(ts(250)));
    replay.checkpoint().unwrap();
    let boundary_hash = replay.stable_hash().unwrap();
    replay.seek(ts(150)).unwrap();
    assert_eq!(
        replay.engine().state(ObjectId(1)),
        Some(InteractionState::Active)
    );
    replay.checkpoint().unwrap();
    replay.seek(ts(250)).unwrap();
    assert_eq!(replay.stable_hash().unwrap(), boundary_hash);
    replay.seek(ts(500)).unwrap();
    assert_eq!(replay.stable_hash().unwrap(), terminal_hash);
    assert_eq!(replay.results()[1].at, ts(500));
    let mut baseline = engine();
    let mut results = baseline
        .push_input(&button(1, ButtonState::Down, 1), ts(100))
        .unwrap();
    results.extend(baseline.advance_to(ts(250)).unwrap());
    replay.seek(ts(250)).unwrap();
    assert_eq!(
        replay.engine().stable_hash().unwrap(),
        baseline.stable_hash().unwrap()
    );
    assert_eq!(replay.results(), results);
}
#[test]
fn recorded_order_provenance_and_boundary_fork_are_reusable() {
    let mut replay = ReplaySession::new(header(), engine()).unwrap();
    replay
        .push_input(button(1, ButtonState::Down, 1), ts(100))
        .unwrap();
    replay
        .push_input(button(2, ButtonState::Up, 1), ts(100))
        .unwrap();
    replay
        .push_input(button(1, ButtonState::Up, 2), ts(200))
        .unwrap();
    replay.advance_to(ts(500)).unwrap();
    assert_eq!(
        replay
            .records()
            .iter()
            .map(|record| record.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    let loaded =
        ReplaySession::from_records(header(), engine(), replay.records().iter().cloned()).unwrap();
    assert_eq!(loaded.stable_hash().unwrap(), replay.stable_hash().unwrap());
    assert_eq!(
        loaded.results()[0].input.unwrap().native.unwrap().code,
        Some(42)
    );
    replay.seek(ts(250)).unwrap();
    assert_eq!(
        replay.advance_to(ts(260)).unwrap_err(),
        ReplayError::FutureExists
    );
    replay.fork_at_cursor();
    assert!(matches!(
        replay.records().last().unwrap().operation,
        ReplayOperation::Advance
    ));
    replay.advance_to(ts(260)).unwrap();
    let loaded =
        ReplaySession::from_records(header(), engine(), replay.records().iter().cloned()).unwrap();
    assert_eq!(loaded.stable_hash().unwrap(), replay.stable_hash().unwrap());
}
struct LegacyPolicy;
impl JudgePolicy for LegacyPolicy {
    fn grade(&self, delta: i128, profile: &JudgeProfile) -> Option<JudgeGrade> {
        profile.grade(delta)
    }
}
#[test]
fn custom_implementations_remain_compatible_and_fail_snapshot_explicitly() {
    let judge = JudgeEngine::with_policies(
        chart(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(HoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(1),
                evaluator: Box::new(InstantEvaluator),
            },
        ],
        profile(),
        Box::new(ClosestCandidate),
        Box::new(LegacyPolicy),
    )
    .unwrap();
    assert!(matches!(
        judge.snapshot(),
        Err(SnapshotError::UnsupportedPolicy)
    ));
}
#[test]
fn incompatible_restore_and_rejected_record_leave_state_unchanged() {
    let snapshot = engine().snapshot().unwrap();
    let mut incompatible = JudgeEngine::new(
        chart(),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(2),
                evaluator: Box::new(HoldEvaluator),
            },
            Rule {
                interaction: InteractionId(2),
                control: GameControlId(2),
                evaluator: Box::new(InstantEvaluator),
            },
        ],
        profile(),
    )
    .unwrap();
    let before = incompatible.stable_hash().unwrap();
    assert_eq!(
        incompatible.restore(&snapshot),
        Err(SnapshotError::ConfigurationMismatch)
    );
    assert_eq!(incompatible.stable_hash().unwrap(), before);
    let mut replay = ReplaySession::new(header(), engine()).unwrap();
    let before = replay.stable_hash().unwrap();
    let mut event = button(1, ButtonState::Down, 1);
    event.physical.meta_mut().clock_domain = ClockDomainId(99);
    assert_eq!(
        replay.push_input(event, ts(100)),
        Err(ReplayError::ClockDomainMismatch)
    );
    assert_eq!(replay.stable_hash().unwrap(), before);
}
