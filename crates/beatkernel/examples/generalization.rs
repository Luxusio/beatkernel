//! Six typed input patterns composed through the same runtime and replay judge.
#[path = "generalization/pose_interaction.rs"]
pub mod pose_interaction;
use beatkernel::{
    audio::{command_queue, CommandConsumer},
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        AxisEvent, AxisMode, BackendId, Binding, BindingMap, ButtonEvent, ButtonState, ContactId,
        DeviceId, DeviceSelector, EventMeta, GameControlId, NativeEventMeta, PhysicalControlId,
        PhysicalInputEvent, PointerEvent, PointerMode, PoseEvent, Position2, Position3, Quaternion,
        TouchEvent, TouchPhase,
    },
    interaction::{
        CompositeEvaluator, InstantEvaluator, InteractionEvaluator, InteractionState,
        RepeatedEvaluator, TrackingEvaluator, TrackingInput,
    },
    judge::{
        JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeSnapshot, JudgeStage,
        JudgeWindow, Rule,
    },
    replay::{ReplayHeader, ReplayRecorder, ReplaySession, REPLAY_VERSION},
    runtime::{Runtime, RuntimeReport},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use std::error::Error;

const HOST_ORIGIN: i64 = 1_000_000_000;

pub struct FixtureClocks;
impl ClockMapper for FixtureClocks {
    fn map(&self, point: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        if point.domain == ClockDomainId(1) && to == ClockDomainId(2) {
            point
                .timestamp
                .checked_add(Duration::from_nanos(HOST_ORIGIN))
        } else {
            None
        }
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}

pub struct Fixture {
    pub runtime: Runtime,
    pub recorder: ReplayRecorder,
    origin: JudgeSnapshot,
    _consumer: CommandConsumer,
}
impl Fixture {
    pub fn input(&mut self, event: PhysicalInputEvent) -> Result<RuntimeReport, Box<dyn Error>> {
        let at = event.meta().timestamp;
        let report = self.runtime.process_input(
            event,
            &FixtureClocks,
            ClockPoint {
                domain: ClockDomainId(3),
                timestamp: at,
            },
        )?;
        if let Some(error) = report.judge_error {
            return Err(error.into());
        }
        self.recorder.record_report(&report)?;
        Ok(report)
    }
    pub fn advance(&mut self, nanos: i64) -> Result<RuntimeReport, Box<dyn Error>> {
        let at = Timestamp::from_nanos(nanos);
        let report = self.runtime.advance_to(
            ClockPoint {
                domain: ClockDomainId(1),
                timestamp: at,
            },
            &FixtureClocks,
            ClockPoint {
                domain: ClockDomainId(3),
                timestamp: at,
            },
        )?;
        if let Some(error) = report.judge_error {
            return Err(error.into());
        }
        self.recorder.record_report(&report)?;
        Ok(report)
    }
    pub fn replay_session(&self) -> Result<ReplaySession, Box<dyn Error>> {
        Ok(ReplaySession::from_records(
            self.recorder.header().clone(),
            JudgeEngine::from_snapshot(&self.origin)?,
            self.recorder.records().iter().cloned(),
        )?)
    }
}

pub fn build_fixture() -> Result<Fixture, Box<dyn Error>> {
    build_fixture_with_count(3)
}

pub fn build_fixture_with_count(minimum_hits: u32) -> Result<Fixture, Box<dyn Error>> {
    build_fixture_options(minimum_hits, None)
}

/// Eight objects covering the combined input policies without changing the legacy fixture.
pub fn build_six_pattern_fixture(axis_mode: AxisMode) -> Result<Fixture, Box<dyn Error>> {
    build_fixture_options(3, Some(axis_mode))
}

fn build_fixture_options(
    minimum_hits: u32,
    axis_mode: Option<AxisMode>,
) -> Result<Fixture, Box<dyn Error>> {
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1)?)?;
    let mut specs = vec![
        (1, 100, Some(300)),
        (2, 100, Some(300)),
        (3, 100, Some(300)),
        (4, 100, Some(300)),
        (5, 200, None),
        (6, 100, Some(300)),
        (7, 100, Some(300)),
    ];
    if axis_mode.is_some() {
        specs.push((8, 200, None));
    }
    for (id, start, end) in specs {
        chart.objects.push(SourceObject {
            id: ObjectId(id),
            start: Beat::new(start)?,
            end: end.map(Beat::new).transpose()?,
            interaction: InteractionId(id as u32),
            visual: VisualId(id as u32),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
    }
    let tracking = |input, points| {
        Box::new(TrackingEvaluator {
            input,
            points,
            tolerance: 0.001,
            max_gap: Duration::from_nanos(150),
        }) as Box<dyn InteractionEvaluator>
    };
    let mut rules = vec![
        Rule {
            interaction: InteractionId(1),
            control: GameControlId(1),
            evaluator: tracking(TrackingInput::Axis, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
        },
        Rule {
            interaction: InteractionId(2),
            control: GameControlId(2),
            evaluator: tracking(
                TrackingInput::Contact,
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            ),
        },
        Rule {
            interaction: InteractionId(3),
            control: GameControlId(2),
            evaluator: tracking(
                TrackingInput::Contact,
                vec![[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            ),
        },
        Rule {
            interaction: InteractionId(4),
            control: GameControlId(4),
            evaluator: Box::new(RepeatedEvaluator { minimum_hits }),
        },
        Rule {
            interaction: InteractionId(5),
            control: GameControlId(5),
            evaluator: Box::new(CompositeEvaluator {
                required_controls: vec![GameControlId(51), GameControlId(52)],
                same_device: true,
            }),
        },
        Rule {
            interaction: InteractionId(6),
            control: GameControlId(6),
            evaluator: tracking(
                TrackingInput::Pointer,
                vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            ),
        },
        Rule {
            interaction: InteractionId(7),
            control: GameControlId(7),
            evaluator: if axis_mode.is_some() {
                Box::new(pose_interaction::DerivedPoseEvaluator)
            } else {
                tracking(TrackingInput::Pose, vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]])
            },
        },
    ];
    if axis_mode.is_some() {
        rules.push(Rule {
            interaction: InteractionId(8),
            control: GameControlId(8),
            evaluator: Box::new(InstantEvaluator),
        });
    }
    let judge = JudgeEngine::new(
        chart.compile()?,
        rules,
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(10),
                late: Duration::from_nanos(10),
            }],
            Duration::ZERO,
        )?,
    )?;
    let origin = judge.snapshot()?;
    let (chart_identity, rules_identity, options) = match axis_mode {
        Some(mode) => (
            b"six-pattern-combinations/v2".to_vec(),
            b"tracking-repeated-composite-instant-derived-pose/v2".to_vec(),
            format!(
                "tolerance=0.001;gap=150ns;count={minimum_hits};same_device=true;axis_mode={};pointer_mode=relative;pose=unit-diagonal-identity-cone/v1;pose_abs_w_min=0.9;pose_norm_squared_tolerance=0.001;pointer_button_control=8",
                axis_mode_name(mode)
            ).into_bytes(),
        ),
        None => (
            b"six-input-patterns/v1".to_vec(),
            b"tracking-repeated-composite/v1".to_vec(),
            format!("tolerance=0.001;gap=150ns;count={minimum_hits};same_device=true").into_bytes(),
        ),
    };
    let recorder = ReplayRecorder::new(ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity,
        rules_identity,
        options,
        seed: 19,
        normalized_clock: ClockDomainId(2),
    })?;
    let mut controls = vec![1, 2, 4, 5, 6, 7, 51, 52];
    if axis_mode.is_some() {
        controls.push(8);
    }
    let bindings = BindingMap::from_bindings(controls.into_iter().map(|control| Binding {
        device: DeviceSelector::Any,
        physical: physical(control),
        game_control: GameControlId(control),
    }))?;
    let (producer, consumer) = command_queue(4)?;
    let runtime = Runtime::new(
        ClockDomainId(2),
        ClockDomainId(3),
        Transport::new(
            Timestamp::from_nanos(HOST_ORIGIN),
            Timestamp::ZERO,
            Rate::NORMAL,
        ),
        bindings,
        judge,
        producer,
        vec![],
        64,
    )?;
    Ok(Fixture {
        runtime,
        recorder,
        origin,
        _consumer: consumer,
    })
}

pub fn physical(control: u32) -> PhysicalControlId {
    PhysicalControlId::Native {
        backend: BackendId(9),
        code: control,
    }
}
fn meta(device: u64, control: u32, nanos: i64, sequence: u64) -> EventMeta {
    let point = ClockPoint {
        domain: ClockDomainId(1),
        timestamp: Timestamp::from_nanos(nanos),
    };
    let mut meta = EventMeta::new(DeviceId(device), point, sequence);
    meta.native = Some(NativeEventMeta {
        backend: BackendId(9),
        code: Some(control),
        timestamp: Some(point),
    });
    meta
}
pub fn button(
    device: u64,
    control: u32,
    nanos: i64,
    sequence: u64,
    state: ButtonState,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Button(ButtonEvent {
        meta: meta(device, control, nanos, sequence),
        control: physical(control),
        state,
    })
}
pub fn axis(nanos: i64, sequence: u64, value: f32) -> PhysicalInputEvent {
    PhysicalInputEvent::Axis(AxisEvent {
        meta: meta(10, 1, nanos, sequence),
        control: physical(1),
        value,
        mode: AxisMode::Relative,
    })
}
pub fn contact(
    device: u64,
    id: u64,
    nanos: i64,
    sequence: u64,
    x: f32,
    y: f32,
    phase: TouchPhase,
) -> PhysicalInputEvent {
    PhysicalInputEvent::Touch(TouchEvent {
        meta: meta(device, 2, nanos, sequence),
        control: physical(2),
        contact: ContactId(id),
        phase,
        position: Position2 { x, y },
        pressure: Some(0.8),
    })
}
pub fn pointer(nanos: i64, sequence: u64, delta: f32) -> PhysicalInputEvent {
    PhysicalInputEvent::Pointer(PointerEvent {
        meta: meta(50, 6, nanos, sequence),
        control: physical(6),
        position: Position2 { x: delta, y: delta },
        mode: PointerMode::Relative,
    })
}
pub fn pose(nanos: i64, sequence: u64, position: f32) -> PhysicalInputEvent {
    PhysicalInputEvent::Pose(PoseEvent {
        meta: meta(60, 7, nanos, sequence),
        control: physical(7),
        position: Position3 {
            x: position,
            y: position,
            z: position,
        },
        orientation: Quaternion {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
    })
}

pub fn fixture_events() -> Vec<PhysicalInputEvent> {
    vec![
        button(40, 51, 0, 1, ButtonState::Down),
        button(40, 52, 0, 2, ButtonState::Down),
        axis(100, 1, 0.0),
        contact(20, 101, 100, 1, 0.0, 0.0, TouchPhase::Down),
        contact(20, 102, 100, 2, 0.0, 1.0, TouchPhase::Down),
        pointer(100, 1, 0.0),
        pose(100, 1, 0.0),
        button(30, 4, 100, 1, ButtonState::Down),
        button(30, 4, 110, 2, ButtonState::Repeat),
        button(30, 4, 120, 3, ButtonState::Up),
        button(30, 4, 130, 4, ButtonState::Down),
        button(30, 4, 140, 5, ButtonState::Up),
        button(30, 4, 150, 6, ButtonState::Down),
        axis(200, 2, 0.5),
        contact(20, 101, 200, 3, 0.5, 0.0, TouchPhase::Move),
        contact(20, 102, 200, 4, 0.5, 1.0, TouchPhase::Move),
        pointer(200, 2, 0.5),
        pose(200, 2, 0.5),
        button(40, 5, 200, 3, ButtonState::Down),
        contact(21, 101, 250, 1, 0.75, 0.0, TouchPhase::Up),
        contact(20, 999, 250, 5, 0.75, 0.0, TouchPhase::Up),
        axis(300, 3, 0.5),
        contact(20, 101, 300, 6, 1.0, 0.0, TouchPhase::Up),
        contact(20, 102, 300, 7, 1.0, 1.0, TouchPhase::Up),
        pointer(300, 3, 0.5),
        pose(300, 3, 1.0),
    ]
}

fn axis_mode_name(mode: AxisMode) -> &'static str {
    match mode {
        AxisMode::Absolute => "absolute",
        AxisMode::Relative => "relative",
    }
}

/// The legacy physical trajectory, plus a separate pointer-device button press.
pub fn six_pattern_events(axis_mode: AxisMode) -> Vec<PhysicalInputEvent> {
    let mut events = fixture_events();
    for event in &mut events {
        match event {
            PhysicalInputEvent::Axis(axis) => {
                axis.mode = axis_mode;
                if axis_mode == AxisMode::Absolute && axis.meta.sequence == 3 {
                    axis.value = 1.0;
                }
            }
            PhysicalInputEvent::Pointer(pointer) if pointer.meta.sequence == 3 => {
                pointer.meta.sequence = 4;
            }
            PhysicalInputEvent::Pose(pose) if pose.meta.sequence == 3 => {
                pose.orientation.w = -1.0;
            }
            _ => {}
        }
    }
    let index = events
        .iter()
        .position(|event| matches!(event, PhysicalInputEvent::Pointer(pointer) if pointer.meta.sequence == 2))
        .expect("fixed fixture midpoint pointer sample");
    events.insert(index + 1, button(50, 8, 200, 3, ButtonState::Down));
    events
}

fn run_six_patterns(axis_mode: AxisMode) -> Result<(), Box<dyn Error>> {
    let mut fixture = build_six_pattern_fixture(axis_mode)?;
    let mut live_results = Vec::new();
    for event in six_pattern_events(axis_mode) {
        live_results.extend(fixture.input(event)?.judge_events);
    }
    live_results.extend(fixture.advance(400)?.judge_events);
    if live_results.len() != 8 {
        return Err("six-pattern fixture expected eight literal hits".into());
    }
    for (id, stage, time) in [
        (1, JudgeStage::Custom(0), 300),
        (2, JudgeStage::Custom(0), 300),
        (3, JudgeStage::Custom(0), 300),
        (4, JudgeStage::Custom(0), 150),
        (5, JudgeStage::Custom(0), 200),
        (6, JudgeStage::Custom(0), 300),
        (7, JudgeStage::Custom(0), 300),
        (8, JudgeStage::Instant, 200),
    ] {
        let expected_outcome = JudgeOutcome::Hit {
            grade: JudgeGrade(1),
            delta: Duration::ZERO,
        };
        if fixture.runtime.judge().state(ObjectId(id)) != Some(InteractionState::Completed)
            || !live_results.iter().any(|result| {
                result.object == ObjectId(id)
                    && result.stage == stage
                    && result.at == Timestamp::from_nanos(time)
                    && result.outcome == expected_outcome
                    && result.input.is_some()
            })
        {
            return Err(format!("six-pattern literal outcome differs for object {id}").into());
        }
    }
    let mut replay = fixture.replay_session()?;
    if replay.results() != live_results
        || replay.engine().stable_hash()? != fixture.runtime.judge().stable_hash()?
    {
        return Err("six-pattern live/replay results or state differs".into());
    }
    let final_cursor = replay.cursor();
    let final_hash = replay.stable_hash()?;
    replay.seek_cursor(8)?;
    replay.seek_cursor(final_cursor)?;
    if replay.stable_hash()? != final_hash || replay.results() != live_results {
        return Err("six-pattern replay reconstruction differs".into());
    }
    println!(
        "eight completed objects with eight hits; axis_mode={}; replay_hash={final_hash:016x}; software fixture only",
        axis_mode_name(axis_mode)
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("Usage: cargo run -p beatkernel --example generalization -- [--fixture|--six-patterns|--help]\n--fixture (default): legacy seven-object fixture.\n--six-patterns: eight-object combined policies, absolute and relative axes, pointer button instant, derived orientation-dependent pose; software fixture only.\nSix input patterns: axis, dual contacts, repeated button, chord prerequisites, pointer, pose. Physical input traverses normalization/binding/transport/runtime and is recorded from the live judge and reconstructed by the same judge implementation.");
        return Ok(());
    }
    if args == ["--six-patterns"] {
        run_six_patterns(AxisMode::Absolute)?;
        return run_six_patterns(AxisMode::Relative);
    }
    if !args.is_empty() && args != ["--fixture"] {
        return Err("expected --fixture, --six-patterns or --help".into());
    }
    let mut fixture = build_fixture()?;
    for event in fixture_events() {
        for result in fixture.input(event)?.judge_events {
            println!("{result:?}");
        }
    }
    fixture.advance(400)?;
    let mut replay = fixture.replay_session()?;
    if replay.engine().stable_hash()? != fixture.runtime.judge().stable_hash()? {
        return Err("live/replay state differs".into());
    }
    let final_cursor = replay.cursor();
    let final_hash = replay.stable_hash()?;
    replay.seek_cursor(8)?;
    replay.seek_cursor(final_cursor)?;
    if replay.stable_hash()? != final_hash || replay.results().len() != 7 {
        return Err("generalization replay reconstruction differs".into());
    }
    println!("seven objects across six typed inputs; replay_hash={final_hash:016x}; software fixture only");
    Ok(())
}
