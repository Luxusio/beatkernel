//! Bounded chronological synthetic BMS rendering through the shared runtime.
use crate::{
    PreparedBms, gauge::BmsGauge, input_sounds::InputSoundPlan, mine_plan::prepare_judge,
    mine_sounds::MineSoundPlan,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandPushError, Mixer,
        MixerConfig, RenderReport,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        PhysicalControlId, PhysicalInputEvent, VendorNamespaceId,
    },
    judge::{JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow},
    runtime::{Runtime, RuntimeReport},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{encode_pcm, DeviceFormat, SampleEncoding};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    io::Write,
};

/// Independent output extent and bounded outstanding audio work.
#[derive(Clone, Copy, Debug)]
pub struct OfflineOptions {
    /// Number of output frames; zero performs no grading or writing.
    pub frames: u64,
    /// Maximum frames per render and writer block.
    pub block_frames: usize,
    /// Queue, pending storage and command-drain capacity.
    pub command_capacity: usize,
    /// Maximum concurrent voices, independent of total chart notes.
    pub max_voices: usize,
}

/// Completed offline output and actual stage counts.
#[derive(Clone, Copy, Debug)]
pub struct OfflineReport {
    /// Complete frames written.
    pub frames: u64,
    /// Interleaved float32 output format.
    pub format: AudioFormat,
    /// Number of emitted Hit stages.
    pub hits: usize,
    /// Number of emitted JudgeEvents, including misses.
    pub judge_results: usize,
    /// Most recent successful core render, absent for zero frames.
    pub last_render: Option<RenderReport>,
}

/// Failure evidence; an already written prefix and judged state are not rolled back.
#[derive(Debug)]
pub struct OfflineError {
    /// Failure description, including actual execution counters when relevant.
    pub message: String,
    /// Most recent completed render, including a rejected execution report.
    pub last_render: Option<RenderReport>,
    /// Exact commands rejected at queue admission, with their reasons.
    pub audio_failures: Vec<CommandPushError>,
}
impl fmt::Display for OfflineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; last_render={:?}; audio_failures={:?}",
            self.message, self.last_render, self.audio_failures
        )
    }
}
impl Error for OfflineError {}
fn failure(message: impl ToString, last_render: Option<RenderReport>) -> OfflineError {
    OfflineError {
        message: message.to_string(),
        last_render,
        audio_failures: Vec::new(),
    }
}

/// Actual Stop admissions from one closed queue owner, not execution evidence.
#[derive(Clone, Copy, Default)]
pub(crate) struct OwnedStopEvidence {
    admitted_stops: u64,
}

impl OwnedStopEvidence {
    pub(crate) const fn admitted_stops(&self) -> u64 {
        self.admitted_stops
    }

    pub(crate) const fn permits_unknown_stops(&self, count: u64) -> bool {
        count <= self.admitted_stops
    }

    /// Record each successfully admitted command prefix exactly once.
    /// Planned, requested and rejected commands must never be supplied here.
    /// Overflow preserves all previously recorded evidence.
    pub(crate) fn record_admitted(
        &mut self,
        commands: &[AudioCommand],
    ) -> Result<(), &'static str> {
        let count = commands
            .iter()
            .filter(|command| matches!(command, AudioCommand::Stop { .. }))
            .count();
        let count = u64::try_from(count).map_err(|_| "accepted Stop count overflow")?;
        let next = self
            .admitted_stops
            .checked_add(count)
            .ok_or("accepted Stop count overflow")?;
        self.admitted_stops = next;
        Ok(())
    }
}

const DOMAIN: ClockDomainId = ClockDomainId(0x424d53);
fn point(timestamp: Timestamp) -> ClockPoint {
    ClockPoint {
        domain: DOMAIN,
        timestamp,
    }
}
struct Identity;
impl ClockMapper for Identity {
    fn map(&self, from: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        (from.domain == to).then_some(from.timestamp)
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
fn physical(channel: u8) -> PhysicalControlId {
    PhysicalControlId::Vendor {
        namespace: VendorNamespaceId(0x424d53),
        code: u32::from(channel),
    }
}
#[derive(Clone, Copy)]
enum Item {
    Bgm(AudioCommand),
    Input {
        control: PhysicalControlId,
        state: ButtonState,
    },
    Advance,
}
struct Record {
    at: Timestamp,
    frame: u64,
    ordinal: usize,
    item: Item,
}
fn target_frame(at: Timestamp, rate: u32) -> Result<u64, Box<dyn Error>> {
    if at.as_nanos() < 0 {
        return Err("offline schedule has a negative timestamp".into());
    }
    let scaled = i128::from(at.as_nanos()) * i128::from(rate);
    Ok(u64::try_from((scaled + 999_999_999) / 1_000_000_000)?)
}
fn accept(
    mut report: RuntimeReport,
    summary: &mut OfflineReport,
    gauge: &mut BmsGauge,
    runtime: &mut Runtime,
    admitted_stops: &mut OwnedStopEvidence,
    bgm_collision: bool,
) -> Result<(), OfflineError> {
    let hits = report
        .judge_events
        .iter()
        .filter(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
        .count();
    let mut errors = Vec::new();
    match (
        summary.judge_results.checked_add(report.judge_events.len()),
        summary.hits.checked_add(hits),
    ) {
        (Some(results), Some(hits)) => {
            summary.judge_results = results;
            summary.hits = hits;
        }
        _ => errors.push("offline judge count overflow".to_owned()),
    }
    let was_failed = gauge.snapshot().failure.is_some();
    if let Err(error) = gauge.observe(&report.judge_events, &report.hazard_events) {
        errors.push(error.to_string());
    }
    if !was_failed && gauge.snapshot().failure.is_some() {
        runtime.fence_gameplay();
        if bgm_collision {
            errors.push("gameplay failure Stop voice collides with BGM".to_owned());
        } else if let Some(stops) = runtime.fence_gameplay_sounds(report.audio_at.timestamp) {
            if let Err(error) = admitted_stops.record_admitted(&stops.commands) {
                errors.push(format!("offline {error}"));
            }
            report.audio_commands.extend(stops.commands);
            report.audio_failures.extend(stops.failures);
        }
    }
    if let Some(error) = report.judge_error {
        errors.push(error.to_string());
    }
    if !report.audio_failures.is_empty() {
        errors.push("audio command admission failed".to_owned());
    }
    if !errors.is_empty() {
        return Err(OfflineError {
            message: errors.join("; "),
            last_render: summary.last_render,
            audio_failures: report.audio_failures,
        });
    }
    Ok(())
}
pub(crate) fn render_block(
    mixer: &mut Mixer,
    pcm: &mut [f32],
    bytes: &mut [u8],
    format: DeviceFormat,
    output: &mut dyn Write,
    summary: &mut OfflineReport,
) -> Result<(), OfflineError> {
    render_block_with_stops(
        mixer,
        pcm,
        bytes,
        format,
        output,
        summary,
        &OwnedStopEvidence::default(),
    )
}

// Callers own fresh closed queues and record actual accepted commands once.
// Generic callers retain strict zero allowance through render_block above.
pub(crate) fn render_block_with_stops(
    mixer: &mut Mixer,
    pcm: &mut [f32],
    bytes: &mut [u8],
    format: DeviceFormat,
    output: &mut dyn Write,
    summary: &mut OfflineReport,
    admitted_stops: &OwnedStopEvidence,
) -> Result<(), OfflineError> {
    let report = mixer
        .render(pcm)
        .map_err(|error| failure(error, summary.last_render))?;
    summary.last_render = Some(report);
    let c = report.counters;
    if c.late_commands != 0
        || c.pending_full != 0
        || c.voice_full != 0
        || c.unknown_samples != 0
        || !admitted_stops.permits_unknown_stops(c.unknown_stops)
        || c.invalid_gains != 0
        || c.invalid_rates != 0
        || c.invalid_times != 0
    {
        return Err(failure(
            format!("core audio execution rejected commands: {:?}", c),
            Some(report),
        ));
    }
    let frames = u64::try_from(report.frames)
        .ok()
        .and_then(|frames| summary.frames.checked_add(frames))
        .ok_or_else(|| failure("offline rendered frame count overflow", Some(report)))?;
    encode_pcm(format, pcm, bytes).map_err(|error| failure(error, Some(report)))?;
    output
        .write_all(bytes)
        .map_err(|error| failure(error, Some(report)))?;
    summary.frames = frames;
    Ok(())
}

/// Renders only admitted target frames through the actual Runtime/JudgeEngine/Mixer.
///
/// Synthetic input preserves exact compiled times; overlapping lane/hold patterns
/// retain real judge outcomes. Same-frame capacity overflow is explicit. The sink
/// is not flushed; any failure may leave a prefix. No native playback occurs.
/// Numeric gauge failure fences gameplay and schedules its owned voice Stops;
/// independent BGM and the requested output extent continue without a clear claim.
pub fn render_offline(
    prepared: PreparedBms,
    options: OfflineOptions,
    output: &mut dyn Write,
) -> Result<OfflineReport, Box<dyn Error>> {
    let format = prepared.bank.format();
    let rate = format.sample_rate();
    let channels = usize::from(format.channels());
    let limits = AudioLimits::new(
        options.command_capacity,
        options.max_voices,
        options.command_capacity,
        options.block_frames,
        options.command_capacity,
    )?;
    let extent_ns =
        (i128::from(options.frames) * 1_000_000_000 + i128::from(rate) - 1) / i128::from(rate);
    i64::try_from(extent_ns).map_err(|_| "offline duration exceeds representable timestamps")?;
    options
        .frames
        .checked_mul(channels as u64)
        .and_then(|samples| samples.checked_mul(4))
        .ok_or("offline output byte extent overflow")?;
    let encoded = DeviceFormat::new(rate, format.channels(), SampleEncoding::Float32, None)?;
    let input_sounds = if prepared.source.invisible.is_empty() {
        None
    } else {
        let plan = InputSoundPlan::prepare(
            &prepared.source,
            &prepared.sounds,
            &prepared.bgm_commands,
            beatkernel_bms::ParseOptions::default().max_objects,
        )?;
        for &sample in plan.samples() {
            if prepared.bank.get(sample).is_none() {
                return Err("offline input sound sample is missing from PCM bank".into());
            }
        }
        Some(plan.timeline())
    };
    let hazard_sounds = if prepared.source.mines.is_empty() {
        None
    } else {
        let plan = MineSoundPlan::prepare(
            &prepared.source,
            &prepared.sounds,
            &prepared.bgm_commands,
            input_sounds.as_ref(),
            beatkernel_bms::ParseOptions::default().max_objects,
        )?;
        for &sample in plan.samples() {
            if prepared.bank.get(sample).is_none() {
                return Err("offline mine sound sample is missing from PCM bank".into());
            }
        }
        plan.timeline()
    };
    let mines = if prepared.source.mines.is_empty() {
        Vec::new()
    } else {
        prepared.source.compile_mines()?
    };
    let bgm_collision = if mines.is_empty() {
        false
    } else {
        let voices: BTreeSet<_> = prepared
            .sounds
            .iter()
            .map(|sound| sound.voice)
            .chain(
                input_sounds
                    .iter()
                    .flat_map(|timeline| timeline.markers().iter().map(|marker| marker.voice)),
            )
            .chain(
                hazard_sounds
                    .iter()
                    .flat_map(|timeline| timeline.bindings().iter().map(|binding| binding.voice)),
            )
            .collect();
        prepared.bgm_commands.iter().any(|command| match command {
            AudioCommand::Play { voice, .. } => voices.contains(voice),
            _ => false,
        })
    };
    let count = options
        .block_frames
        .checked_mul(channels)
        .ok_or("offline block sample extent overflow")?;
    let mut pcm = Vec::new();
    pcm.try_reserve_exact(count)?;
    pcm.resize(count, 0.0);
    let mut bytes = Vec::new();
    let byte_count = count
        .checked_mul(4)
        .ok_or("offline block byte extent overflow")?;
    bytes.try_reserve_exact(byte_count)?;
    bytes.resize(byte_count, 0);
    let objects: BTreeMap<_, _> = prepared
        .compiled
        .chart
        .objects()
        .iter()
        .map(|object| (object.id, object.time))
        .collect();
    let schedule_len = prepared
        .source
        .notes
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_add(prepared.bgm_commands.len()))
        .and_then(|n| n.checked_add(mines.len()))
        .ok_or("offline schedule extent overflow")?;
    let mut records = Vec::new();
    records.try_reserve_exact(schedule_len)?;
    let mut lanes = BTreeMap::new();
    for event in &prepared.source.invisible {
        lanes.insert(event.lane.channel(), event.lane.control());
    }
    for (ordinal, mine) in mines.iter().enumerate() {
        lanes.insert(mine.lane.channel(), mine.lane.control());
        if ordinal != 0 && mines[ordinal - 1].at == mine.at {
            continue;
        }
        records.push(Record {
            at: mine.at,
            frame: target_frame(mine.at, rate)?,
            ordinal,
            item: Item::Advance,
        });
    }
    for (ordinal, note) in prepared.source.notes.iter().enumerate() {
        let time = objects
            .get(&note.object)
            .ok_or("source note has no compiled object")?;
        lanes.insert(note.lane.channel(), note.lane.control());
        for (index, (at, state)) in [
            (time.start, ButtonState::Down),
            (time.end.unwrap_or(time.start), ButtonState::Up),
        ]
        .into_iter()
        .enumerate()
        {
            records.push(Record {
                at,
                frame: target_frame(at, rate)?,
                ordinal: ordinal * 2 + index,
                item: Item::Input {
                    control: physical(note.lane.channel()),
                    state,
                },
            });
        }
    }
    for (ordinal, command) in prepared.bgm_commands.iter().copied().enumerate() {
        if !matches!(command, AudioCommand::Play { .. }) {
            return Err("prepared BGM must contain Play commands".into());
        }
        records.push(Record {
            at: command.at(),
            frame: target_frame(command.at(), rate)?,
            ordinal,
            item: Item::Bgm(command),
        });
    }
    records.sort_by_key(|record| {
        (
            record.at,
            match record.item {
                Item::Bgm(_) => 0u8,
                Item::Input { .. } => 1,
                Item::Advance => 2,
            },
            record.ordinal,
        )
    });
    let bindings =
        BindingMap::from_bindings(lanes.into_iter().map(|(channel, game_control)| Binding {
            device: DeviceSelector::Exact(DeviceId(1)),
            physical: physical(channel),
            game_control,
        }))?;
    let judge = prepare_judge(
        &prepared.source,
        prepared.compiled.chart,
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(1_000_000),
                late: Duration::from_nanos(1_000_000),
            }],
            Duration::ZERO,
        )?,
        beatkernel_bms::BmsInputMode::ButtonOnly,
        beatkernel_bms::ParseOptions::default().max_objects,
    )?;
    let (producer, consumer) = command_queue(options.command_capacity)?;
    let mut runtime = Runtime::new(
        DOMAIN,
        DOMAIN,
        Transport::new(Timestamp::ZERO, Timestamp::ZERO, Rate::NORMAL),
        bindings,
        judge,
        producer,
        prepared.sounds,
        256,
    )?;
    if let Some(timeline) = input_sounds {
        runtime.configure_input_sounds(timeline)?;
    }
    if let Some(timeline) = hazard_sounds {
        runtime.configure_hazard_sounds(timeline)?;
    }
    let mut mixer = Mixer::new(
        MixerConfig::new(format, DOMAIN, Timestamp::ZERO, limits),
        prepared.bank,
        consumer,
    )?;
    let mut summary = OfflineReport {
        frames: 0,
        format,
        hits: 0,
        judge_results: 0,
        last_render: None,
    };
    let mut index = 0;
    let mut sequence = 0u64;
    let mut gauge = BmsGauge::default();
    let mut admitted_stops = OwnedStopEvidence::default();
    while summary.frames < options.frames {
        let next_frame = records
            .get(index)
            .map_or(options.frames, |record| record.frame.min(options.frames));
        if next_frame > summary.frames {
            let frames =
                usize::try_from((next_frame - summary.frames).min(options.block_frames as u64))?;
            render_block_with_stops(
                &mut mixer,
                &mut pcm[..frames * channels],
                &mut bytes[..frames * channels * 4],
                encoded,
                output,
                &mut summary,
                &admitted_stops,
            )?;
        } else {
            while let Some(record) = records
                .get(index)
                .filter(|record| record.frame == summary.frames)
            {
                match record.item {
                    Item::Bgm(command) => {
                        runtime
                            .enqueue_audio(command)
                            .map_err(|error| OfflineError {
                                message: "BGM command admission failed".into(),
                                last_render: summary.last_render,
                                audio_failures: vec![error],
                            })?
                    }
                    Item::Input { control, state } => {
                        sequence = sequence
                            .checked_add(1)
                            .ok_or("synthetic acquisition sequence overflow")?;
                        let event = PhysicalInputEvent::Button(ButtonEvent {
                            meta: EventMeta::new(DeviceId(1), point(record.at), sequence),
                            control,
                            state,
                        });
                        let report = runtime
                            .process_input(event, &Identity, point(record.at))
                            .map_err(|error| failure(error, summary.last_render))?;
                        accept(
                            report,
                            &mut summary,
                            &mut gauge,
                            &mut runtime,
                            &mut admitted_stops,
                            bgm_collision,
                        )?;
                    }
                    Item::Advance => {
                        let report = runtime
                            .advance_to(point(record.at), &Identity, point(record.at))
                            .map_err(|error| failure(error, summary.last_render))?;
                        accept(
                            report,
                            &mut summary,
                            &mut gauge,
                            &mut runtime,
                            &mut admitted_stops,
                            bgm_collision,
                        )?;
                    }
                }
                index += 1;
            }
            // The next iteration renders this admitted frame group before another group.
        }
    }
    if options.frames != 0 {
        let final_ns =
            i64::try_from(i128::from(options.frames - 1) * 1_000_000_000 / i128::from(rate))?;
        let at = point(Timestamp::from_nanos(final_ns));
        let report = runtime
            .advance_to(at, &Identity, at)
            .map_err(|error| failure(error, summary.last_render))?;
        accept(
            report,
            &mut summary,
            &mut gauge,
            &mut runtime,
            &mut admitted_stops,
            bgm_collision,
        )?;
    }
    Ok(summary)
}
