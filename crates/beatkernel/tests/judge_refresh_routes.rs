//! Public-operation regressions for refresh route preparation. Callback logs
//! belong to this correctness fixture, never to the allocation measurement.

use std::sync::{Arc, Mutex};

use beatkernel::{
    chart::{
        compile, Beat, Bpm, CompiledChart, InteractionId, ObjectId, ObjectMetadata, SourceChart,
        SourceObject, TimedObject, VisualId,
    },
    input::{
        AxisEvent, AxisMode, BackendId, DeviceId, EventMeta, GameControlId, GameInputEvent,
        NativeEventMeta, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::{
        ActiveInteraction, BeginContext, InteractionContext, InteractionEvaluator,
        InteractionOutput, InteractionResult, InteractionState,
    },
    judge::{
        JudgeEngine, JudgeError, JudgeEvent, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeStage,
        JudgeWindow, MissReason, Rule,
    },
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};

const PRIMARY: GameControlId = GameControlId(7);
const A: GameControlId = GameControlId(11);
const B: GameControlId = GameControlId(19);
const UNRELATED: GameControlId = GameControlId(23);
const OBJECT: ObjectId = ObjectId(41);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Additional,
    State(InteractionState),
    Deadline(Option<i128>),
    Accept(GameControlId, InteractionState, bool),
    Input(GameControlId, InteractionState, i64),
    Advance(InteractionState, i64),
}

type Calls = Arc<Mutex<Vec<Call>>>;

#[derive(Clone, Copy)]
enum Mode {
    Lifecycle,
    Deadline { successor: i128 },
}

struct Evaluator {
    additional: Vec<GameControlId>,
    calls: Calls,
    mode: Mode,
}

struct Interaction {
    additional: Vec<GameControlId>,
    calls: Calls,
    mode: Mode,
    state: InteractionState,
    deadline: Option<i128>,
    advances: usize,
}

impl InteractionEvaluator for Evaluator {
    fn validate(&self, _: &TimedObject, _: &JudgeProfile) -> Result<(), JudgeError> {
        Ok(())
    }

    fn begin(&self, _: &TimedObject, _: &BeginContext<'_>) -> Box<dyn ActiveInteraction> {
        Box::new(Interaction {
            additional: self.additional.clone(),
            calls: Arc::clone(&self.calls),
            mode: self.mode,
            state: InteractionState::Pending,
            deadline: Some(100),
            advances: 0,
        })
    }
}

impl Interaction {
    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }
}

impl ActiveInteraction for Interaction {
    fn additional_controls(&self) -> &[GameControlId] {
        self.record(Call::Additional);
        &self.additional
    }

    fn state(&self) -> InteractionState {
        self.record(Call::State(self.state));
        self.state
    }

    fn deadline(&self, _: &JudgeProfile) -> Option<i128> {
        self.record(Call::Deadline(self.deadline));
        self.deadline
    }

    fn accepts_input(&self, event: &GameInputEvent, _: &InteractionContext<'_>) -> bool {
        // Deliberately independent of logical control: the real engine route
        // index, rather than this predicate, must reject unrelated controls.
        let accepted = matches!(self.mode, Mode::Lifecycle)
            && self.state != InteractionState::Completed
            && matches!(&event.physical, PhysicalInputEvent::Axis(axis) if axis.value >= 0.0);
        self.record(Call::Accept(event.game_control, self.state, accepted));
        accepted
    }

    fn on_input(
        &mut self,
        event: &GameInputEvent,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        self.record(Call::Input(
            event.game_control,
            self.state,
            context.song_time.as_nanos(),
        ));
        let PhysicalInputEvent::Axis(axis) = &event.physical else {
            panic!("fixture accepts only axis samples");
        };
        let stage = if self.state == InteractionState::Pending {
            self.state = InteractionState::Active;
            31
        } else if axis.value == 2.0 {
            self.state = InteractionState::Completed;
            33
        } else {
            32
        };
        self.deadline = None;
        InteractionOutput {
            results: vec![InteractionResult {
                stage: JudgeStage::Custom(stage),
                outcome: JudgeOutcome::Hit {
                    grade: JudgeGrade(17),
                    delta: Duration::ZERO,
                },
            }],
        }
    }

    fn advance_to(
        &mut self,
        time: Timestamp,
        context: &InteractionContext<'_>,
    ) -> InteractionOutput {
        assert_eq!(time, context.song_time);
        self.record(Call::Advance(self.state, time.as_nanos()));
        let Mode::Deadline { successor } = self.mode else {
            return InteractionOutput::default();
        };
        self.advances += 1;
        // A broken consumed-deadline filter fails promptly instead of hanging
        // this test, including a filter that reschedules an equal deadline.
        assert!(self.advances <= if successor > 100 { 2 } else { 1 });
        if self.advances == 1 {
            self.deadline = Some(successor);
            InteractionOutput::default()
        } else {
            self.state = InteractionState::Completed;
            self.deadline = None;
            InteractionOutput {
                results: vec![InteractionResult {
                    stage: JudgeStage::Custom(34),
                    outcome: JudgeOutcome::Miss {
                        reason: MissReason::HeadTimeout,
                    },
                }],
            }
        }
    }
}

struct Fixture {
    engine: JudgeEngine,
    calls: Calls,
}

impl Fixture {
    fn new(additional: &[GameControlId], mode: Mode) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let profile = JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(17),
                early: Duration::ZERO,
                late: Duration::ZERO,
            }],
            Duration::ZERO,
        )
        .unwrap();
        let engine = JudgeEngine::new(
            chart(),
            vec![Rule {
                interaction: InteractionId(5),
                control: PRIMARY,
                evaluator: Box::new(Evaluator {
                    additional: additional.to_vec(),
                    calls: Arc::clone(&calls),
                    mode,
                }),
            }],
            profile,
        )
        .unwrap();
        // Cold build performs canonicalization after refresh. Its getter calls
        // are outside each public transition observed below.
        calls.lock().unwrap().clear();
        Self { engine, calls }
    }

    fn push(&mut self, input: &GameInputEvent, at: i64) -> (Vec<JudgeEvent>, Vec<Call>) {
        self.calls.lock().unwrap().clear();
        let results = self
            .engine
            .push_input(input, Timestamp::from_nanos(at))
            .unwrap();
        let calls = std::mem::take(&mut *self.calls.lock().unwrap());
        (results, calls)
    }

    fn advance(&mut self, at: i64) -> (Vec<JudgeEvent>, Vec<Call>) {
        self.calls.lock().unwrap().clear();
        let results = self.engine.advance_to(Timestamp::from_nanos(at)).unwrap();
        let calls = std::mem::take(&mut *self.calls.lock().unwrap());
        (results, calls)
    }

    fn assert_state(&self, expected: InteractionState) {
        // Call only after push/advance has captured the operation log.
        assert_eq!(self.engine.state(OBJECT), Some(expected));
    }
}

fn chart() -> CompiledChart {
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1).unwrap()).unwrap();
    source.objects.push(SourceObject {
        id: OBJECT,
        start: Beat::new(100).unwrap(),
        end: None,
        interaction: InteractionId(5),
        visual: VisualId(0),
        audio: None,
        metadata: ObjectMetadata::default(),
    });
    compile(&source).unwrap()
}

fn meta(sequence: u64) -> EventMeta {
    let point = ClockPoint {
        domain: ClockDomainId(91),
        timestamp: Timestamp::from_nanos(-700),
    };
    let mut meta = EventMeta::new(DeviceId(13), point, sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(29),
        code: Some(0xfeed),
        timestamp: Some(point),
    });
    meta.original_clock_point = Some(ClockPoint {
        domain: ClockDomainId(92),
        timestamp: Timestamp::from_nanos(800),
    });
    meta
}

fn input(control: GameControlId, value: f32, sequence: u64) -> GameInputEvent {
    GameInputEvent {
        game_control: control,
        physical: PhysicalInputEvent::Axis(AxisEvent {
            meta: meta(sequence),
            control: PhysicalControlId::keyboard(4),
            value,
            mode: AxisMode::Relative,
        }),
    }
}

fn hit(stage: u32, at: i64, sequence: u64) -> Vec<JudgeEvent> {
    vec![JudgeEvent {
        object: OBJECT,
        stage: JudgeStage::Custom(stage),
        outcome: JudgeOutcome::Hit {
            grade: JudgeGrade(17),
            delta: Duration::ZERO,
        },
        at: Timestamp::from_nanos(at),
        input: Some(meta(sequence)),
    }]
}

fn refresh_calls(
    state: InteractionState,
    unique_routes: usize,
    deadline: Option<i128>,
) -> Vec<Call> {
    // This focused regression preserves existing refresh getter effects; it
    // does not establish a permanent public API call-count guarantee. State is
    // observed once per unique pending route and at both lifecycle branches.
    let mut calls = vec![Call::Additional];
    calls.extend(std::iter::repeat_n(Call::State(state), unique_routes + 2));
    if state != InteractionState::Completed {
        calls.push(Call::Deadline(deadline));
    }
    calls
}

fn route_cases() -> [(&'static [GameControlId], &'static [GameControlId]); 3] {
    [
        (&[], &[PRIMARY]),
        (&[PRIMARY, PRIMARY], &[PRIMARY]),
        (&[A, PRIMARY, A, B, B], &[PRIMARY, A, B]),
    ]
}

#[test]
fn pending_and_active_routes_preserve_duplicates_lifecycle_and_provenance() {
    for (additional, routes) in route_cases() {
        // Separate cold fixtures prove Pending admission from every unique
        // additional route, independently of later Active dispatch.
        for &start in routes {
            let mut fixture = Fixture::new(additional, Mode::Lifecycle);
            assert_eq!(
                fixture.push(&input(UNRELATED, 0.0, 1), 99),
                (vec![], vec![])
            );
            assert_eq!(
                fixture.push(&input(start, -1.0, 2), 99),
                (
                    vec![],
                    vec![
                        Call::State(InteractionState::Pending),
                        Call::Deadline(Some(100)),
                        Call::Accept(start, InteractionState::Pending, false),
                    ],
                )
            );
            fixture.assert_state(InteractionState::Pending);
            assert_eq!(fixture.advance(100), (vec![], vec![]));

            let (results, calls) = fixture.push(&input(start, 0.0, 3), 100);
            let mut expected = vec![
                Call::State(InteractionState::Pending),
                Call::Deadline(Some(100)),
                Call::Accept(start, InteractionState::Pending, true),
                Call::State(InteractionState::Pending),
                Call::Accept(start, InteractionState::Pending, true),
                Call::Input(start, InteractionState::Pending, 100),
            ];
            expected.extend(refresh_calls(InteractionState::Active, routes.len(), None));
            assert_eq!(
                calls, expected,
                "additional={additional:?}, start={start:?}"
            );
            assert_eq!(results, hit(31, 100, 3));
            fixture.assert_state(InteractionState::Active);

            assert_eq!(
                fixture.push(&input(UNRELATED, 1.0, 4), 100),
                (vec![], vec![])
            );
            assert_eq!(
                fixture.push(&input(start, -1.0, 5), 100),
                (
                    vec![],
                    vec![
                        Call::State(InteractionState::Active),
                        Call::Accept(start, InteractionState::Active, false),
                    ],
                )
            );
            for (index, &control) in routes.iter().enumerate() {
                let at = 101 + index as i64;
                let sequence = 10 + index as u64;
                let (results, calls) = fixture.push(&input(control, 1.0, sequence), at);
                let mut expected = vec![
                    Call::State(InteractionState::Active),
                    Call::Accept(control, InteractionState::Active, true),
                    Call::Input(control, InteractionState::Active, at),
                ];
                expected.extend(refresh_calls(InteractionState::Active, routes.len(), None));
                assert_eq!(calls, expected, "active control={control:?}");
                assert_eq!(results, hit(32, at, sequence));
                fixture.assert_state(InteractionState::Active);
            }

            // Active interactions legitimately receive later time callbacks.
            let (results, calls) = fixture.advance(120);
            let mut expected = vec![Call::Advance(InteractionState::Active, 120)];
            expected.extend(refresh_calls(InteractionState::Active, routes.len(), None));
            assert_eq!(calls, expected);
            assert!(results.is_empty());
            fixture.assert_state(InteractionState::Active);

            let (results, calls) = fixture.push(&input(start, 2.0, 30), 121);
            let mut expected = vec![
                Call::State(InteractionState::Active),
                Call::Accept(start, InteractionState::Active, true),
                Call::Input(start, InteractionState::Active, 121),
            ];
            expected.extend(refresh_calls(
                InteractionState::Completed,
                routes.len(),
                None,
            ));
            assert_eq!(calls, expected);
            assert_eq!(results, hit(33, 121, 30));
            fixture.assert_state(InteractionState::Completed);
            for &control in routes {
                assert_eq!(
                    fixture.push(&input(control, 0.0, 31), 122),
                    (vec![], vec![])
                );
            }
            assert_eq!(fixture.advance(200), (vec![], vec![]));
            fixture.assert_state(InteractionState::Completed);
        }
    }
}

#[test]
fn input_at_inclusive_deadline_can_precede_equal_time_advance() {
    let mut fixture = Fixture::new(&[], Mode::Lifecycle);
    let (results, calls) = fixture.push(&input(PRIMARY, 0.0, 90), 100);
    assert_eq!(results, hit(31, 100, 90));
    assert!(!calls.iter().any(|call| matches!(call, Call::Advance(..))));
    fixture.assert_state(InteractionState::Active);
    let (results, calls) = fixture.advance(100);
    let mut expected = vec![Call::Advance(InteractionState::Active, 100)];
    expected.extend(refresh_calls(InteractionState::Active, 1, None));
    assert_eq!(calls, expected);
    assert!(results.is_empty());
    fixture.assert_state(InteractionState::Active);
}

#[test]
fn pending_deadline_successor_is_scheduled_and_equality_does_not_expire() {
    for (additional, routes) in route_cases() {
        let mut fixture = Fixture::new(additional, Mode::Deadline { successor: 200 });
        assert_eq!(fixture.advance(100), (vec![], vec![]));
        fixture.assert_state(InteractionState::Pending);
        let (results, calls) = fixture.advance(101);
        let mut expected = vec![
            Call::State(InteractionState::Pending),
            Call::Deadline(Some(100)),
            Call::Advance(InteractionState::Pending, 101),
        ];
        expected.extend(refresh_calls(
            InteractionState::Pending,
            routes.len(),
            Some(200),
        ));
        assert_eq!(calls, expected);
        assert!(results.is_empty());
        fixture.assert_state(InteractionState::Pending);
        assert_eq!(fixture.advance(200), (vec![], vec![]));
        fixture.assert_state(InteractionState::Pending);

        let (results, calls) = fixture.advance(201);
        let mut expected = vec![
            Call::State(InteractionState::Pending),
            Call::Deadline(Some(200)),
            Call::Advance(InteractionState::Pending, 201),
        ];
        expected.extend(refresh_calls(
            InteractionState::Completed,
            routes.len(),
            None,
        ));
        assert_eq!(calls, expected);
        assert_eq!(
            results,
            vec![JudgeEvent {
                object: OBJECT,
                stage: JudgeStage::Custom(34),
                outcome: JudgeOutcome::Miss {
                    reason: MissReason::HeadTimeout
                },
                at: Timestamp::from_nanos(201),
                input: None,
            }]
        );
        fixture.assert_state(InteractionState::Completed);
        assert_eq!(fixture.advance(202), (vec![], vec![]));
        for &control in routes {
            assert_eq!(
                fixture.push(&input(control, 0.0, 91), 202),
                (vec![], vec![])
            );
        }
    }
}

#[test]
fn equal_and_backward_consumed_deadlines_cannot_repeat_pending_callbacks() {
    for successor in [100, 99] {
        for (additional, routes) in route_cases() {
            let mut fixture = Fixture::new(additional, Mode::Deadline { successor });
            assert_eq!(fixture.advance(100), (vec![], vec![]));
            let (results, calls) = fixture.advance(101);
            let mut expected = vec![
                Call::State(InteractionState::Pending),
                Call::Deadline(Some(100)),
                Call::Advance(InteractionState::Pending, 101),
            ];
            expected.extend(refresh_calls(
                InteractionState::Pending,
                routes.len(),
                Some(successor),
            ));
            assert_eq!(calls, expected, "successor={successor}");
            assert!(results.is_empty());
            fixture.assert_state(InteractionState::Pending);
            // Unlike Active callbacks, this still-Pending faulty interaction
            // has no usable successor and must not be re-expired indefinitely.
            assert_eq!(fixture.advance(102), (vec![], vec![]));
            assert_eq!(fixture.advance(1_000), (vec![], vec![]));
            fixture.assert_state(InteractionState::Pending);
        }
    }
}
