//! Deferred opt-in contact interactions over the actual judge and replay owners.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use beatkernel::{
    chart::*,
    input::*,
    interaction::*,
    judge::*,
    replay::{REPLAY_VERSION, ReplayHeader, ReplaySession},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

fn ts(value: i64) -> Timestamp {
    Timestamp::from_nanos(value)
}
fn dur(value: i64) -> Duration {
    Duration::from_nanos(value)
}
fn profile(offset: i64) -> JudgeProfile {
    JudgeProfile::new(
        vec![
            JudgeWindow {
                grade: JudgeGrade(7),
                early: dur(5),
                late: dur(10),
            },
            JudgeWindow {
                grade: JudgeGrade(9),
                early: dur(20),
                late: dur(30),
            },
        ],
        dur(offset),
    )
    .unwrap()
}
fn chart(objects: &[(u64, i64, Option<i64>, u32)]) -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects = objects
        .iter()
        .map(|&(id, start, end, interaction)| SourceObject {
            id: ObjectId(id),
            start: Beat::new(start).unwrap(),
            end: end.map(|value| Beat::new(value).unwrap()),
            interaction: InteractionId(interaction),
            visual: VisualId(0),
            audio: None,
            metadata: ObjectMetadata::default(),
        })
        .collect();
    compile(&source).unwrap()
}
fn rules(press: bool) -> Vec<Rule> {
    vec![
        Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: if press {
                Box::new(PressInstantEvaluator)
            } else {
                Box::new(InstantEvaluator)
            },
        },
        Rule {
            interaction: InteractionId(2),
            control: GameControlId(1),
            evaluator: if press {
                Box::new(PressHoldEvaluator)
            } else {
                Box::new(HoldEvaluator)
            },
        },
        Rule {
            interaction: InteractionId(3),
            control: GameControlId(2),
            evaluator: if press {
                Box::new(PressInstantEvaluator)
            } else {
                Box::new(InstantEvaluator)
            },
        },
    ]
}
fn engine(objects: &[(u64, i64, Option<i64>, u32)], press: bool, offset: i64) -> JudgeEngine {
    JudgeEngine::new(chart(objects), rules(press), profile(offset)).unwrap()
}
fn meta(device: u64) -> EventMeta {
    EventMeta {
        source: DeviceId(device),
        timestamp: ts(9_007_199_254_740_993),
        clock_domain: ClockDomainId(42),
        sequence: u64::MAX,
        native: Some(NativeEventMeta {
            backend: BackendId(7),
            code: Some(u32::MAX),
            timestamp: Some(ClockPoint {
                domain: ClockDomainId(43),
                timestamp: ts(-1),
            }),
        }),
        original_clock_point: Some(ClockPoint {
            domain: ClockDomainId(44),
            timestamp: ts(-99),
        }),
    }
}
fn surface(code: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(0x544f_5543),
        code,
    }
}
fn touch(
    device: u64,
    surface_id: u32,
    game: u32,
    contact: u64,
    phase: TouchPhase,
) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(game),
        physical: PhysicalInputEvent::Touch(TouchEvent {
            meta: meta(device),
            control: surface(surface_id),
            contact: ContactId(contact),
            phase,
            position: Position2 {
                x: -123.25,
                y: 65536.5,
            },
            pressure: Some(0.625),
        }),
    }
}
fn button(device: u64, surface_id: u32, game: u32, state: ButtonState) -> GameInputEvent {
    GameInputEvent {
        game_control: GameControlId(game),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: meta(device),
            control: surface(surface_id),
            state,
        }),
    }
}
fn assert_hit(
    events: &[JudgeEvent],
    id: u64,
    stage: JudgeStage,
    grade: u32,
    delta: i64,
    at: i64,
    input: &GameInputEvent,
) {
    assert_eq!(
        events,
        &[JudgeEvent {
            object: ObjectId(id),
            stage,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(grade),
                delta: dur(delta)
            },
            at: ts(at),
            input: Some(*input.physical.meta())
        }]
    );
}

#[test]
fn fresh_button_and_contact_presses_are_independent_and_matching_release_rearms_each_identity() {
    let objects = [
        (1, 100, None, 1),
        (2, 100, None, 1),
        (3, 100, None, 1),
        (4, 100, None, 1),
        (5, 100, None, 1),
        (6, 100, None, 1),
        (7, 100, None, 3),
    ];
    let mut judge = engine(&objects, true, 0);
    let first = touch(1, 5, 1, 10, TouchPhase::Down);
    assert_hit(
        &judge.push_input(&first, ts(100)).unwrap(),
        1,
        JudgeStage::Instant,
        7,
        0,
        100,
        &first,
    );
    for event in [first.clone(), touch(1, 5, 1, 10, TouchPhase::Move)] {
        assert!(judge.push_input(&event, ts(100)).unwrap().is_empty());
    }
    let key = button(1, 5, 1, ButtonState::Down);
    assert_hit(
        &judge.push_input(&key, ts(100)).unwrap(),
        2,
        JudgeStage::Instant,
        7,
        0,
        100,
        &key,
    );
    assert!(judge.push_input(&key, ts(100)).unwrap().is_empty());
    assert!(
        judge
            .push_input(&button(1, 5, 1, ButtonState::Repeat), ts(100))
            .unwrap()
            .is_empty()
    );
    let second = touch(1, 5, 1, 11, TouchPhase::Down);
    assert_hit(
        &judge.push_input(&second, ts(100)).unwrap(),
        3,
        JudgeStage::Instant,
        7,
        0,
        100,
        &second,
    );
    assert!(
        judge
            .push_input(&touch(1, 5, 1, 99, TouchPhase::Up), ts(100))
            .unwrap()
            .is_empty()
    );
    assert!(judge.push_input(&first, ts(100)).unwrap().is_empty());
    assert!(
        judge
            .push_input(&touch(1, 5, 1, 10, TouchPhase::Cancel), ts(100))
            .unwrap()
            .is_empty()
    );
    assert_hit(
        &judge.push_input(&first, ts(100)).unwrap(),
        4,
        JudgeStage::Instant,
        7,
        0,
        100,
        &first,
    );
    judge
        .push_input(&button(1, 5, 1, ButtonState::Up), ts(100))
        .unwrap();
    assert_hit(
        &judge.push_input(&key, ts(100)).unwrap(),
        5,
        JudgeStage::Instant,
        7,
        0,
        100,
        &key,
    );
    judge
        .push_input(&touch(1, 5, 1, 11, TouchPhase::Up), ts(100))
        .unwrap();
    assert_hit(
        &judge.push_input(&second, ts(100)).unwrap(),
        6,
        JudgeStage::Instant,
        7,
        0,
        100,
        &second,
    );
    let fanout = touch(1, 5, 2, 10, TouchPhase::Down);
    assert_hit(
        &judge.push_input(&fanout, ts(100)).unwrap(),
        7,
        JudgeStage::Instant,
        7,
        0,
        100,
        &fanout,
    );
    assert!(judge.advance_to(ts(1000)).unwrap().is_empty());
}

#[test]
fn hold_release_requires_the_original_device_surface_lane_contact_and_kind_while_cancel_is_a_miss()
{
    for phase in [TouchPhase::Up, TouchPhase::Cancel] {
        let mut judge = engine(&[(1, 100, Some(200), 2)], true, 0);
        let down = touch(u64::MAX, 5, 1, u64::MAX, TouchPhase::Down);
        assert_hit(
            &judge.push_input(&down, ts(100)).unwrap(),
            1,
            JudgeStage::HoldHead,
            7,
            0,
            100,
            &down,
        );
        let mut moved = touch(u64::MAX, 5, 1, u64::MAX, TouchPhase::Move);
        if let PhysicalInputEvent::Touch(event) = &mut moved.physical {
            event.position = Position2 { x: 0.0, y: 0.0 };
            event.pressure = None;
        }
        for event in [
            touch(0, 5, 1, u64::MAX, phase),
            touch(u64::MAX, 6, 1, u64::MAX, phase),
            touch(u64::MAX, 5, 2, u64::MAX, phase),
            touch(u64::MAX, 5, 1, 0, phase),
            button(u64::MAX, 5, 1, ButtonState::Up),
            moved,
            down.clone(),
        ] {
            assert!(judge.push_input(&event, ts(200)).unwrap().is_empty());
            assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Active));
        }
        let release = touch(u64::MAX, 5, 1, u64::MAX, phase);
        let result = judge.push_input(&release, ts(200)).unwrap();
        if phase == TouchPhase::Up {
            assert_hit(&result, 1, JudgeStage::HoldTail, 7, 0, 200, &release);
        } else {
            assert_eq!(
                result,
                vec![JudgeEvent {
                    object: ObjectId(1),
                    stage: JudgeStage::HoldTail,
                    outcome: JudgeOutcome::Miss {
                        reason: MissReason::RejectedInput
                    },
                    at: ts(200),
                    input: Some(*release.physical.meta())
                }]
            );
        }
        assert_eq!(judge.state(ObjectId(1)), Some(InteractionState::Completed));
        assert!(judge.push_input(&release, ts(200)).unwrap().is_empty());
        assert!(judge.advance_to(ts(1000)).unwrap().is_empty());
    }
    let mut judge = engine(&[(1, 100, Some(200), 2)], true, 0);
    let down = button(1, 5, 1, ButtonState::Down);
    judge.push_input(&down, ts(100)).unwrap();
    assert!(
        judge
            .push_input(&touch(1, 5, 1, 0, TouchPhase::Cancel), ts(200))
            .unwrap()
            .is_empty()
    );
    let up = button(1, 5, 1, ButtonState::Up);
    assert_hit(
        &judge.push_input(&up, ts(200)).unwrap(),
        1,
        JudgeStage::HoldTail,
        7,
        0,
        200,
        &up,
    );
    for (at, reason) in [
        (230, MissReason::RejectedInput),
        (231, MissReason::TailTimeout),
    ] {
        let mut judge = engine(&[(1, 100, Some(200), 2)], true, 0);
        judge
            .push_input(&touch(1, 5, 1, 7, TouchPhase::Down), ts(100))
            .unwrap();
        let cancel = touch(1, 5, 1, 7, TouchPhase::Cancel);
        assert_eq!(
            judge.push_input(&cancel, ts(at)).unwrap(),
            vec![JudgeEvent {
                object: ObjectId(1),
                stage: JudgeStage::HoldTail,
                outcome: JudgeOutcome::Miss { reason },
                at: ts(at),
                input: if at == 230 {
                    Some(*cancel.physical.meta())
                } else {
                    None
                },
            }],
            "cancellation at the inclusive deadline is input; expired holds already timed out"
        );
    }
}

#[test]
fn contact_timing_uses_inclusive_profile_windows_and_one_offset_with_checked_long_time_failures() {
    for (at, grade, delta) in [
        (480, 9, -20),
        (495, 7, -5),
        (500, 7, 0),
        (510, 7, 10),
        (530, 9, 30),
    ] {
        let mut judge = engine(&[(1, 500, None, 1)], true, 100);
        let input = touch(1, 1, 1, 7, TouchPhase::Down);
        assert_hit(
            &judge.push_input(&input, ts(at - 100)).unwrap(),
            1,
            JudgeStage::Instant,
            grade,
            delta,
            at,
            &input,
        );
        assert_eq!(judge.effective_song_time(), Some(ts(at)));
    }
    for at in [479, 531] {
        let mut judge = engine(&[(1, 500, None, 1)], true, 100);
        let mut events = judge
            .push_input(&touch(1, 1, 1, 7, TouchPhase::Down), ts(at - 100))
            .unwrap();
        events.extend(judge.advance_to(ts(431)).unwrap());
        assert_eq!(
            events,
            vec![JudgeEvent {
                object: ObjectId(1),
                stage: JudgeStage::Instant,
                outcome: JudgeOutcome::Miss {
                    reason: MissReason::HeadTimeout
                },
                at: ts(531),
                input: None
            }]
        );
    }
    for target in [604_800_000_000_001, i64::MAX - 10] {
        let mut judge = engine(&[(1, target, None, 1)], true, 7);
        let input = touch(1, 1, 1, 7, TouchPhase::Down);
        assert_hit(
            &judge.push_input(&input, ts(target + 3)).unwrap(),
            1,
            JudgeStage::Instant,
            7,
            10,
            target + 10,
            &input,
        );
        let before = judge.stable_hash().unwrap();
        assert_eq!(
            judge.push_input(&input, ts(target + 2)),
            Err(JudgeError::NonMonotonicSongTime)
        );
        assert_eq!(judge.stable_hash().unwrap(), before);
    }
    let mut judge = engine(&[(1, 100, None, 1)], true, 1);
    let input = touch(1, 1, 1, 7, TouchPhase::Down);
    let before = judge.stable_hash().unwrap();
    assert_eq!(
        judge.push_input(&input, ts(i64::MAX)),
        Err(JudgeError::Overflow)
    );
    assert_eq!(judge.stable_hash().unwrap(), before);
    assert_hit(
        &judge.push_input(&input, ts(99)).unwrap(),
        1,
        JudgeStage::Instant,
        7,
        0,
        100,
        &input,
    );
    for objects in [vec![(1, 100, Some(200), 1)], vec![(1, 100, None, 2)]] {
        assert!(matches!(
            JudgeEngine::new(chart(&objects), rules(true), profile(0)),
            Err(JudgeError::InvalidObjectRange {
                object: ObjectId(1)
            })
        ));
    }
}

#[test]
fn checkpoints_and_recorded_operations_preserve_contact_owners_with_canonical_hashes_and_configuration_refusal()
 {
    let objects = [(1, 100, Some(200), 2), (2, 200, None, 1)];
    let down = touch(1, 5, 1, 7, TouchPhase::Down);
    let up = touch(1, 5, 1, 7, TouchPhase::Up);
    let mut judge = engine(&objects, true, 0);
    judge.push_input(&down, ts(100)).unwrap();
    let snapshot = judge.snapshot().unwrap();
    let held_hash = judge.stable_hash().unwrap();
    let mut clone = JudgeEngine::from_snapshot(&snapshot).unwrap();
    assert_eq!(clone.stable_hash().unwrap(), held_hash);
    assert!(
        clone.push_input(&down, ts(200)).unwrap().is_empty(),
        "restored held contact cannot acquire a second head"
    );
    let tail = clone.push_input(&up, ts(200)).unwrap();
    let next = clone.push_input(&down, ts(200)).unwrap();
    assert_hit(&tail, 1, JudgeStage::HoldTail, 7, 0, 200, &up);
    assert_hit(&next, 2, JudgeStage::Instant, 7, 0, 200, &down);
    clone.restore(&snapshot).unwrap();
    assert_eq!(clone.stable_hash().unwrap(), held_hash);
    assert!(
        clone
            .push_input(&touch(1, 5, 1, 8, TouchPhase::Up), ts(200))
            .unwrap()
            .is_empty()
    );
    assert_eq!(clone.push_input(&up, ts(200)).unwrap(), tail);
    assert_eq!(clone.push_input(&down, ts(200)).unwrap(), next);
    for mut incompatible in [engine(&objects, false, 0), engine(&objects, true, 1)] {
        let before = incompatible.stable_hash().unwrap();
        assert_eq!(
            incompatible.restore(&snapshot),
            Err(SnapshotError::ConfigurationMismatch)
        );
        assert_eq!(incompatible.stable_hash().unwrap(), before);
    }
    assert_ne!(
        engine(&objects, true, 0).stable_hash().unwrap(),
        engine(&objects, false, 0).stable_hash().unwrap()
    );
    let header = ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"contact/chart".to_vec(),
        rules_identity: b"contact/press/v1".to_vec(),
        options: vec![],
        seed: u64::MAX,
        normalized_clock: ClockDomainId(42),
    };
    let mut replay = ReplaySession::new(header.clone(), engine(&objects, true, 0)).unwrap();
    replay.push_input(down.clone(), ts(100)).unwrap();
    replay.checkpoint().unwrap();
    replay
        .push_input(touch(1, 5, 1, 7, TouchPhase::Move), ts(150))
        .unwrap();
    replay.push_input(up, ts(200)).unwrap();
    replay.push_input(down, ts(200)).unwrap();
    replay.advance_to(ts(201)).unwrap();
    let results = replay.results().to_vec();
    let terminal = replay.stable_hash().unwrap();
    let rebuilt = ReplaySession::from_records(
        header,
        engine(&objects, true, 0),
        replay.records().iter().cloned(),
    )
    .unwrap();
    assert_eq!(rebuilt.results(), results);
    assert_eq!(rebuilt.stable_hash().unwrap(), terminal);
    replay.seek(ts(100)).unwrap();
    assert_eq!(replay.engine().stable_hash().unwrap(), held_hash);
    replay.seek(ts(201)).unwrap();
    assert_eq!(replay.results(), results);
    assert_eq!(replay.stable_hash().unwrap(), terminal);
    let future = [(1, 1000, None, 1)];
    let mut left = engine(&future, true, 0);
    let mut right = engine(&future, true, 0);
    for id in [u64::MAX, 0, 1] {
        left.push_input(&touch(1, 5, 1, id, TouchPhase::Down), ts(0))
            .unwrap();
    }
    for id in [1, 0, u64::MAX] {
        right
            .push_input(&touch(1, 5, 1, id, TouchPhase::Down), ts(0))
            .unwrap();
    }
    assert_eq!(
        left.stable_hash().unwrap(),
        right.stable_hash().unwrap(),
        "canonical contact ownership is independent of insertion order"
    );
    right
        .push_input(&touch(1, 5, 1, 2, TouchPhase::Down), ts(0))
        .unwrap();
    assert_ne!(left.stable_hash().unwrap(), right.stable_hash().unwrap());
}

struct CountedPress(Arc<AtomicUsize>);
struct ButtonDeclaredPress(Arc<AtomicUsize>);
struct CountedInteraction {
    inner: Box<dyn ActiveInteraction>,
    calls: Arc<AtomicUsize>,
}
impl InteractionEvaluator for CountedPress {
    fn start_eligibility(&self) -> StartEligibility {
        PressInstantEvaluator.start_eligibility()
    }
    fn validate(&self, object: &TimedObject, profile: &JudgeProfile) -> Result<(), JudgeError> {
        PressInstantEvaluator.validate(object, profile)
    }
    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        Box::new(CountedInteraction {
            inner: PressInstantEvaluator.begin(object, context),
            calls: self.0.clone(),
        })
    }
}
impl InteractionEvaluator for ButtonDeclaredPress {
    fn start_eligibility(&self) -> StartEligibility {
        StartEligibility::ProfileButtonPress
    }
    fn validate(&self, object: &TimedObject, profile: &JudgeProfile) -> Result<(), JudgeError> {
        PressInstantEvaluator.validate(object, profile)
    }
    fn begin(
        &self,
        object: &TimedObject,
        context: &BeginContext<'_>,
    ) -> Box<dyn ActiveInteraction> {
        // Deliberately broad predicate: indexed eligibility must still enforce the declaration.
        CountedPress(self.0.clone()).begin(object, context)
    }
}
impl ActiveInteraction for CountedInteraction {
    fn snapshot_clone(&self) -> Option<Box<dyn ActiveInteraction>> {
        Some(Box::new(Self {
            inner: self.inner.snapshot_clone()?,
            calls: self.calls.clone(),
        }))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        self.inner.snapshot_bytes()
    }
    fn state(&self) -> InteractionState {
        self.inner.state()
    }
    fn accepts_input(&self, event: &GameInputEvent, context: &InteractionContext<'_>) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.accepts_input(event, context)
    }
    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.inner.on_input(event, context)
    }
    fn advance_to(
        &mut self,
        time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.inner.advance_to(time, context)
    }
    fn deadline(&self, profile: &JudgeProfile) -> Option<i128> {
        self.inner.deadline(profile)
    }
}

#[test]
fn profile_press_limits_predicate_work_to_indexed_time_candidates_and_fresh_contacts() {
    assert_eq!(
        PressInstantEvaluator.start_eligibility(),
        StartEligibility::ProfilePress
    );
    assert_eq!(
        PressHoldEvaluator.start_eligibility(),
        StartEligibility::ProfilePress
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let mut objects: Vec<_> = (0..4096)
        .map(|index| (index as u64 + 1, 1_000_000 + index * 1000, None, 1))
        .collect();
    objects.push((99_999, 3_000_000, None, 1));
    let mut judge = JudgeEngine::new(
        chart(&objects),
        vec![Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: Box::new(CountedPress(calls.clone())),
        }],
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        judge
            .push_input(&touch(1, 1, 1, 99, TouchPhase::Down), ts(0))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        calls.load(Ordering::Relaxed),
        0,
        "future notes outside the indexed start window never run predicates"
    );
    let input = touch(1, 1, 1, 100, TouchPhase::Down);
    let events = judge.push_input(&input, ts(3_000_000)).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
            .map(|event| event.object)
            .collect::<Vec<_>>(),
        vec![ObjectId(2001)]
    );
    assert!(
        (1..=6).contains(&calls.load(Ordering::Relaxed)),
        "only the two equal-time candidates and selected interaction are consulted"
    );
    calls.store(0, Ordering::Relaxed);
    assert!(judge.push_input(&input, ts(3_000_000)).unwrap().is_empty());
    assert!(
        judge
            .push_input(&touch(1, 1, 1, 100, TouchPhase::Move), ts(3_000_000))
            .unwrap()
            .is_empty()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    judge
        .push_input(&touch(1, 1, 1, 100, TouchPhase::Up), ts(3_000_000))
        .unwrap();
    assert_hit(
        &judge.push_input(&input, ts(3_000_000)).unwrap(),
        99_999,
        JudgeStage::Instant,
        1,
        0,
        3_000_000,
        &input,
    );
    assert!((1..=4).contains(&calls.load(Ordering::Relaxed)));

    let button_calls = Arc::new(AtomicUsize::new(0));
    let press_calls = Arc::new(AtomicUsize::new(0));
    let mut mixed = JudgeEngine::new(
        chart(&[(1, 100, None, 1), (2, 100, None, 3)]),
        vec![
            Rule {
                interaction: InteractionId(1),
                control: GameControlId(1),
                evaluator: Box::new(ButtonDeclaredPress(button_calls.clone())),
            },
            Rule {
                interaction: InteractionId(3),
                control: GameControlId(1),
                evaluator: Box::new(CountedPress(press_calls.clone())),
            },
        ],
        profile(0),
    )
    .unwrap();
    let contact = touch(1, 5, 1, 7, TouchPhase::Down);
    assert_hit(
        &mixed.push_input(&contact, ts(100)).unwrap(),
        2,
        JudgeStage::Instant,
        7,
        0,
        100,
        &contact,
    );
    assert_eq!(
        button_calls.load(Ordering::Relaxed),
        0,
        "touch never consults a ProfileButtonPress predicate, even if it would accept contact"
    );
    assert!(press_calls.load(Ordering::Relaxed) > 0);
    let key = button(1, 5, 1, ButtonState::Down);
    assert_hit(
        &mixed.push_input(&key, ts(100)).unwrap(),
        1,
        JudgeStage::Instant,
        7,
        0,
        100,
        &key,
    );
    assert!(button_calls.load(Ordering::Relaxed) > 0);
}

#[test]
fn legacy_button_interactions_keep_literal_snapshot_schema_and_ignore_contact_state() {
    assert_eq!(
        InstantEvaluator.start_eligibility(),
        StartEligibility::ProfileButtonPress
    );
    assert_eq!(
        HoldEvaluator.start_eligibility(),
        StartEligibility::ProfileButtonPress
    );
    let objects = [(1, 100, None, 1), (2, 200, Some(300), 2)];
    let compiled = chart(&objects);
    let timing = profile(0);
    let pending = InstantEvaluator.begin(
        &compiled.objects()[0],
        &BeginContext {
            control: GameControlId(1),
            profile: &timing,
        },
    );
    let mut literal = vec![21, 0, 0, 0, 0, 0, 0, 0];
    literal.extend_from_slice(b"button-interaction/v1");
    literal.extend_from_slice(&[100, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
    assert_eq!(pending.snapshot_bytes().unwrap(), literal);
    let press = PressInstantEvaluator.begin(
        &compiled.objects()[0],
        &BeginContext {
            control: GameControlId(1),
            profile: &timing,
        },
    );
    assert_ne!(press.snapshot_bytes().unwrap(), literal);
    let mut legacy = engine(&objects, false, 0);
    let mut untouched = engine(&objects, false, 0);
    untouched.advance_to(ts(100)).unwrap();
    for phase in [
        TouchPhase::Down,
        TouchPhase::Move,
        TouchPhase::Up,
        TouchPhase::Cancel,
    ] {
        assert!(
            legacy
                .push_input(&touch(1, 5, 1, u64::MAX, phase), ts(100))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            legacy.stable_hash().unwrap(),
            untouched.stable_hash().unwrap(),
            "legacy complete state has no contact-owner extension or mutation"
        );
    }
    let down = button(1, 5, 1, ButtonState::Down);
    assert_hit(
        &legacy.push_input(&down, ts(100)).unwrap(),
        1,
        JudgeStage::Instant,
        7,
        0,
        100,
        &down,
    );
    assert!(legacy.push_input(&down, ts(200)).unwrap().is_empty());
    assert!(
        legacy
            .push_input(&button(1, 5, 1, ButtonState::Repeat), ts(200))
            .unwrap()
            .is_empty()
    );
    legacy
        .push_input(&button(1, 5, 1, ButtonState::Up), ts(200))
        .unwrap();
    assert_hit(
        &legacy.push_input(&down, ts(200)).unwrap(),
        2,
        JudgeStage::HoldHead,
        7,
        0,
        200,
        &down,
    );
    assert!(
        legacy
            .push_input(&touch(1, 5, 1, 0, TouchPhase::Cancel), ts(300))
            .unwrap()
            .is_empty()
    );
    let up = button(1, 5, 1, ButtonState::Up);
    assert_hit(
        &legacy.push_input(&up, ts(300)).unwrap(),
        2,
        JudgeStage::HoldTail,
        7,
        0,
        300,
        &up,
    );
    assert!(legacy.advance_to(ts(1000)).unwrap().is_empty());
}
