//! Six typed input patterns composed through the same runtime and replay judge.
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
        CompositeEvaluator, InteractionEvaluator, RepeatedEvaluator, TrackingEvaluator,
        TrackingInput,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeSnapshot, JudgeWindow, Rule},
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
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1)?)?;
    let specs = [
        (1, 100, Some(300)),
        (2, 100, Some(300)),
        (3, 100, Some(300)),
        (4, 100, Some(300)),
        (5, 200, None),
        (6, 100, Some(300)),
        (7, 100, Some(300)),
    ];
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
    let rules = vec![
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
            evaluator: tracking(TrackingInput::Pose, vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]),
        },
    ];
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
    let recorder = ReplayRecorder::new(ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"six-input-patterns/v1".to_vec(),
        rules_identity: b"tracking-repeated-composite/v1".to_vec(),
        options: format!("tolerance=0.001;gap=150ns;count={minimum_hits};same_device=true")
            .into_bytes(),
        seed: 19,
        normalized_clock: ClockDomainId(2),
    })?;
    let bindings = BindingMap::from_bindings([1, 2, 4, 5, 6, 7, 51, 52].map(|control| Binding {
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

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("Usage: cargo run -p beatkernel --example generalization -- [--fixture|--help]\nSix input patterns: axis, dual contacts, repeated button, chord prerequisites, pointer, pose. Physical input traverses normalization/binding/transport/runtime and is recorded from the live judge and reconstructed by the same judge implementation.");
        return Ok(());
    }
    if !args.is_empty() && args != ["--fixture"] {
        return Err("expected --fixture or --help".into());
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
