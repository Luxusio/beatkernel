//! Bounded software stress through the production judge, replay and projector.
//! Long origins place timestamps; they do not execute a wall-clock soak.

use std::{error::Error, io, time::Instant};

use beatkernel::{chart::*, input::*, interaction::*, judge::*, replay::*, time::*, visual::*};

type Outcome<T> = Result<T, Box<dyn Error>>;
const ROW_NS: i64 = 4_000_000;
const TAIL_NS: i64 = 2_000_000;
const UNIT_NS: i64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub notes: usize,
    pub lanes: usize,
    pub seek_cycles: usize,
    pub origin_ns: i64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            notes: 20_000,
            lanes: 8,
            seek_cycles: 16,
            origin_ns: 0,
        }
    }
}

fn failure(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

impl Options {
    pub fn validate(&self) -> Outcome<()> {
        if !(1..=100_000).contains(&self.notes) {
            return Err(failure("notes must be in 1..=100000"));
        }
        if !(1..=64).contains(&self.lanes) {
            return Err(failure("lanes must be in 1..=64"));
        }
        if !(1..=1000).contains(&self.seek_cycles) {
            return Err(failure("seek_cycles must be in 1..=1000"));
        }
        if !(0..=604_800_000_000_000).contains(&self.origin_ns) {
            return Err(failure("origin_ns must be in 0..=604800000000000"));
        }
        let work = self
            .seek_cycles
            .checked_add(1)
            .and_then(|cycles| self.notes.checked_mul(cycles))
            .ok_or_else(|| failure("work budget arithmetic overflow"))?;
        if work > 5_000_000 {
            return Err(failure("notes * (seek_cycles + 1) must not exceed 5000000"));
        }
        self.notes
            .checked_mul(2)
            .ok_or_else(|| failure("record count overflow"))?;
        self.lanes
            .checked_mul(4)
            .ok_or_else(|| failure("frame capacity overflow"))?;
        // Admit all fixture and query timestamps before allocating any fixture.
        row_head(*self, (self.notes - 1) / self.lanes)?
            .checked_add(7_000_000)
            .ok_or_else(|| failure("query timestamp overflow"))?;
        Ok(())
    }
}

fn row_head(options: Options, row: usize) -> Outcome<i64> {
    i64::try_from(row)
        .ok()
        .and_then(|row| row.checked_mul(ROW_NS))
        .and_then(|offset| options.origin_ns.checked_add(offset))
        .ok_or_else(|| failure("row timestamp overflow"))
}

fn metadata(lane: usize, at: i64, ordinal: usize) -> Outcome<EventMeta> {
    let point = ClockPoint {
        domain: ClockDomainId(8),
        timestamp: Timestamp::from_nanos(at),
    };
    let sequence = u64::try_from(ordinal)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| failure("input sequence overflow"))?;
    let mut meta = EventMeta::new(
        DeviceId(1),
        ClockPoint {
            domain: ClockDomainId(7),
            timestamp: Timestamp::from_nanos(at),
        },
        sequence,
    );
    meta.native = Some(NativeEventMeta {
        backend: BackendId(2),
        code: Some(u32::try_from(
            lane.checked_add(4)
                .ok_or_else(|| failure("native code overflow"))?,
        )?),
        timestamp: Some(point),
    });
    meta.original_clock_point = Some(point);
    Ok(meta)
}

fn input(lane: usize, at: i64, ordinal: usize, state: ButtonState) -> Outcome<GameInputEvent> {
    Ok(GameInputEvent {
        game_control: GameControlId(u32::try_from(lane + 1)?),
        physical: PhysicalInputEvent::Button(ButtonEvent {
            meta: metadata(lane, at, ordinal)?,
            control: PhysicalControlId::keyboard(u16::try_from(lane + 4)?),
            state,
        }),
    })
}

pub struct Generated {
    pub chart: CompiledChart,
    pub records: Vec<ReplayRecord>,
    pub hold_count: usize,
}

pub fn generate(options: Options) -> Outcome<Generated> {
    options.validate()?;
    let mut source = SourceChart::new(1_000_000_000, Bpm::new(60, 1)?)?;
    source.objects.reserve(options.notes);
    let mut records = Vec::with_capacity(
        options
            .notes
            .checked_mul(2)
            .ok_or_else(|| failure("record count overflow"))?,
    );
    let mut hold_count = 0usize;
    let rows = options.notes.div_ceil(options.lanes);
    for row in 0..rows {
        let head = row_head(options, row)?;
        let tail = head
            .checked_add(TAIL_NS)
            .ok_or_else(|| failure("tail timestamp overflow"))?;
        let first = row
            .checked_mul(options.lanes)
            .ok_or_else(|| failure("row index overflow"))?;
        let count = options.lanes.min(options.notes - first);
        let hold = row.is_multiple_of(4);
        for lane in 0..count {
            source.objects.push(SourceObject {
                id: ObjectId(u64::try_from(first + lane + 1)?),
                start: Beat::new(head)?,
                end: if hold { Some(Beat::new(tail)?) } else { None },
                interaction: InteractionId(u32::try_from(2 * lane + if hold { 2 } else { 1 })?),
                visual: VisualId(u32::try_from(lane + 1)?),
                audio: None,
                metadata: ObjectMetadata::default(),
            });
        }
        if hold {
            hold_count = hold_count
                .checked_add(count)
                .ok_or_else(|| failure("hold count overflow"))?;
        }
        // A chord is acquired completely before any owning release, including
        // instant releases required to make the next row's press fresh.
        for (at, state) in [(head, ButtonState::Down), (tail, ButtonState::Up)] {
            for lane in 0..count {
                let ordinal = records.len();
                records.push(ReplayRecord {
                    ordinal: u64::try_from(ordinal)?,
                    song_time: Timestamp::from_nanos(at),
                    operation: ReplayOperation::Input(input(lane, at, ordinal, state)?),
                });
            }
        }
    }
    Ok(Generated {
        chart: compile(&source)?,
        records,
        hold_count,
    })
}

pub fn make_engine(chart: &CompiledChart, lanes: usize) -> Outcome<JudgeEngine> {
    if !(1..=64).contains(&lanes) {
        return Err(failure("lanes must be in 1..=64"));
    }
    let mut rules = Vec::with_capacity(lanes * 2);
    for lane in 0..lanes {
        rules.push(Rule {
            interaction: InteractionId((2 * lane + 1) as u32),
            control: GameControlId((lane + 1) as u32),
            evaluator: Box::new(PressInstantEvaluator),
        });
        rules.push(Rule {
            interaction: InteractionId((2 * lane + 2) as u32),
            control: GameControlId((lane + 1) as u32),
            evaluator: Box::new(PressHoldEvaluator),
        });
    }
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::ZERO,
            late: Duration::ZERO,
        }],
        Duration::ZERO,
    )?;
    Ok(JudgeEngine::new(chart.clone(), rules, profile)?)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub target_ns: i64,
    pub cursor: usize,
    pub result_count: usize,
    pub engine_hash: u64,
    pub objects: Vec<RenderObjectState>,
}

#[derive(Debug)]
pub struct Report {
    pub options: Options,
    pub hold_count: usize,
    pub record_count: usize,
    pub result_count: usize,
    pub checkpoint_count: usize,
    pub seek_checks: usize,
    pub projection_checks: usize,
    pub max_visible: usize,
    pub final_engine_hash: u64,
    pub probes: Vec<Probe>,
    pub setup_ns: u128,
    pub record_ns: u128,
    pub seek_ns: u128,
    pub projection_ns: u128,
    pub verification_ns: u128,
}

fn header() -> ReplayHeader {
    ReplayHeader {
        version: REPLAY_VERSION,
        chart_identity: b"beatkernel-dense-chart-v1".to_vec(),
        rules_identity: b"press-zero-window-v1".to_vec(),
        options: Vec::new(),
        seed: 0,
        normalized_clock: ClockDomainId(7),
    }
}

fn verify_complete(
    options: Options,
    generated: &Generated,
    session: &ReplaySession,
) -> Outcome<()> {
    if session.records() != generated.records || session.cursor() != generated.records.len() {
        return Err(failure(
            "complete recording/cursor differs from original inputs",
        ));
    }
    let expected_count = options
        .notes
        .checked_add(generated.hold_count)
        .ok_or_else(|| failure("result count overflow"))?;
    if session.results().len() != expected_count {
        return Err(failure("complete result count differs from notes + holds"));
    }
    let mut result_index = 0;
    let mut ordinal = 0;
    for row in 0..options.notes.div_ceil(options.lanes) {
        let first = row * options.lanes;
        let count = options.lanes.min(options.notes - first);
        let head = row_head(options, row)?;
        let hold = row.is_multiple_of(4);
        for (at, state) in [(head, ButtonState::Down), (head + TAIL_NS, ButtonState::Up)] {
            for lane in 0..count {
                // Golden sequence is constructed from source coordinates and
                // chord order, never from actual judge output or a second seek.
                let expected_input = input(lane, at, ordinal, state)?;
                let record = &session.records()[ordinal];
                if record.ordinal != ordinal as u64
                    || record.song_time != Timestamp::from_nanos(at)
                    || record.operation != ReplayOperation::Input(expected_input.clone())
                {
                    return Err(failure("golden input identity/control/provenance mismatch"));
                }
                if state == ButtonState::Down || hold {
                    let expected = JudgeEvent {
                        object: ObjectId((first + lane + 1) as u64),
                        stage: if !hold {
                            JudgeStage::Instant
                        } else if state == ButtonState::Down {
                            JudgeStage::HoldHead
                        } else {
                            JudgeStage::HoldTail
                        },
                        outcome: JudgeOutcome::Hit {
                            grade: JudgeGrade(1),
                            delta: Duration::ZERO,
                        },
                        at: Timestamp::from_nanos(at),
                        input: Some(*expected_input.physical.meta()),
                    };
                    if session.results()[result_index] != expected {
                        return Err(failure(
                            "complete golden result stage/identity/time/grade/provenance mismatch",
                        ));
                    }
                    result_index += 1;
                }
                ordinal += 1;
            }
        }
    }
    for index in 0..options.notes {
        if session.engine().state(ObjectId((index + 1) as u64)) != Some(InteractionState::Completed)
        {
            return Err(failure("complete recording left an object unfinished"));
        }
    }
    for lane in 0..options.lanes {
        if session.engine().is_held(InputOwner {
            source: DeviceId(1),
            physical: PhysicalControlId::keyboard((lane + 4) as u16),
            game_control: GameControlId((lane + 1) as u32),
        }) {
            return Err(failure("complete recording retained an input owner"));
        }
    }
    if session.checkpoints().len() != 3
        || session
            .checkpoints()
            .iter()
            .any(|c| c.boundary_time().is_some())
    {
        return Err(failure("expected only origin/mid/end normal checkpoints"));
    }
    Ok(())
}

fn verify_seek(
    generated: &Generated,
    lanes: usize,
    target: Timestamp,
    session: &ReplaySession,
) -> Outcome<u64> {
    let mut fresh = make_engine(&generated.chart, lanes)?;
    let mut results = Vec::new();
    let mut cursor = 0;
    // Linear fresh prefix reconstruction is deliberately independent of the
    // session's checkpoint choice and indexed partition implementation.
    for record in &generated.records {
        if record.song_time > target {
            break;
        }
        let output = match &record.operation {
            ReplayOperation::Input(event) => fresh.push_input(event, record.song_time)?,
            ReplayOperation::Advance => fresh.advance_to(record.song_time)?,
        };
        results.extend(output);
        cursor += 1;
    }
    if cursor == 0 || generated.records[cursor - 1].song_time < target {
        results.extend(fresh.advance_to(target)?);
    }
    let hash = session.engine().stable_hash()?;
    if cursor != session.cursor() || results != session.results() || hash != fresh.stable_hash()? {
        return Err(failure(
            "seek differs from fresh input-prefix judge: cursor/results/engine hash",
        ));
    }
    Ok(hash)
}

fn verify_frame(
    options: Options,
    target: i64,
    start: i64,
    end: i64,
    frame: &RenderFrame,
) -> Outcome<()> {
    let mut visible = 0;
    for index in 0..options.notes {
        let row = index / options.lanes;
        let head = row_head(options, row)?;
        let tail = if row.is_multiple_of(4) {
            Some(head + TAIL_NS)
        } else {
            None
        };
        if head <= end && tail.unwrap_or(head) >= start {
            let expected = RenderObjectState::Lane {
                object: ObjectId((index + 1) as u64),
                lane: (index % options.lanes) as u32,
                distance: (head - target) as f64 / UNIT_NS as f64,
                tail_distance: tail.map(|tail| (tail - target) as f64 / UNIT_NS as f64),
            };
            if frame.objects.get(visible) != Some(&expected) {
                return Err(failure(
                    "indexed projection differs from independent overlap/geometry oracle",
                ));
            }
            visible += 1;
        }
    }
    if frame.objects.len() != visible
        || frame.song_time != Timestamp::from_nanos(target)
        || !frame.transient_events.is_empty()
    {
        return Err(failure(
            "projection frame identity/count/transient mismatch",
        ));
    }
    for object in &frame.objects {
        match object {
            RenderObjectState::Lane {
                distance,
                tail_distance,
                ..
            } if distance.is_finite() && tail_distance.is_none_or(|tail| tail.is_finite()) => {}
            _ => {
                return Err(failure(
                    "projection contains nonfinite or unexpected geometry",
                ))
            }
        }
    }
    Ok(())
}

pub fn run(options: Options) -> Outcome<Report> {
    options.validate()?;
    let setup = Instant::now();
    let generated = generate(options)?;
    let mut session = ReplaySession::new(header(), make_engine(&generated.chart, options.lanes)?)?;
    let projector = VisualProjector::new(
        generated.chart.clone(),
        (0..options.lanes)
            .map(|lane| VisualBinding {
                id: VisualId((lane + 1) as u32),
                projection: Projection::Lane {
                    lane: lane as u32,
                    unit: Duration::from_nanos(UNIT_NS),
                },
            })
            .collect(),
    )?;
    // An 8ms inclusive window intersects at most three 4ms rows, with at
    // most one additional earlier hold row. Four rows per lane is sufficient.
    let mut frame = RenderFrame {
        objects: Vec::with_capacity(options.lanes * 4),
        ..RenderFrame::default()
    };
    let pointer = frame.objects.as_ptr();
    let capacity = frame.objects.capacity();
    let mut probes = Vec::with_capacity(options.seek_cycles);
    let setup_ns = setup.elapsed().as_nanos();

    let recording = Instant::now();
    for (index, record) in generated.records.iter().enumerate() {
        match &record.operation {
            ReplayOperation::Input(event) => {
                session.push_input(event.clone(), record.song_time)?;
            }
            ReplayOperation::Advance => {
                session.advance_to(record.song_time)?;
            }
        }
        if index + 1 == options.notes {
            session.checkpoint()?;
        }
    }
    session.checkpoint()?;
    let record_ns = recording.elapsed().as_nanos();
    let verification = Instant::now();
    verify_complete(options, &generated, &session)?;
    let final_engine_hash = session.engine().stable_hash()?;
    let result_count = session.results().len();
    let mut verification_ns = verification.elapsed().as_nanos();
    let mut seek_ns = 0;
    let mut projection_ns = 0;
    let mut max_visible = 0;
    let rows = options.notes.div_ceil(options.lanes);
    for cycle in 0..options.seek_cycles {
        let slot = cycle % 5;
        let row = if slot == 4 {
            rows - 1
        } else {
            ((cycle / 5) % rows.div_ceil(4)) * 4
        };
        let offset = if slot == 4 { 3 } else { slot };
        let target_ns = row_head(options, row)?
            .checked_add(i64::try_from(offset)? * UNIT_NS)
            .ok_or_else(|| failure("seek target overflow"))?;
        let target = Timestamp::from_nanos(target_ns);
        let radius = if slot == 2 { 500_000 } else { ROW_NS };
        let start = target_ns
            .checked_sub(radius)
            .ok_or_else(|| failure("window start overflow"))?;
        let end = target_ns
            .checked_add(radius)
            .ok_or_else(|| failure("window end overflow"))?;
        let seeking = Instant::now();
        session.seek(target)?;
        seek_ns += seeking.elapsed().as_nanos();
        let projecting = Instant::now();
        projector.project(
            target,
            Timestamp::from_nanos(start),
            Timestamp::from_nanos(end),
            &[],
            &mut frame,
        )?;
        projection_ns += projecting.elapsed().as_nanos();

        let verification = Instant::now();
        let engine_hash = verify_seek(&generated, options.lanes, target, &session)?;
        verify_frame(options, target_ns, start, end, &frame)?;
        if frame.objects.as_ptr() != pointer || frame.objects.capacity() != capacity {
            return Err(failure(
                "indexed projection grew or moved reusable object storage",
            ));
        }
        if slot == 2
            && !frame.objects.iter().any(|object| {
                matches!(object,
            RenderObjectState::Lane { object, tail_distance: Some(0.0), .. }
                if *object == ObjectId((row * options.lanes + 1) as u64))
            })
        {
            return Err(failure(
                "narrow tail window lost hold whose head is outside window",
            ));
        }
        max_visible = max_visible.max(frame.objects.len());
        probes.push(Probe {
            target_ns,
            cursor: session.cursor(),
            result_count: session.results().len(),
            engine_hash,
            objects: frame.objects.clone(),
        });
        verification_ns += verification.elapsed().as_nanos();
    }
    Ok(Report {
        options,
        hold_count: generated.hold_count,
        record_count: generated.records.len(),
        result_count,
        checkpoint_count: session.checkpoints().len(),
        seek_checks: options.seek_cycles,
        projection_checks: options.seek_cycles,
        max_visible,
        final_engine_hash,
        probes,
        setup_ns,
        record_ns,
        seek_ns,
        projection_ns,
        verification_ns,
    })
}
