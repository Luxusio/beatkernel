//! Bounded chronological synthetic BMS rendering through the shared runtime.
use crate::PreparedBms;
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioFormat, AudioLimits, CommandPushError, Mixer,
        MixerConfig, RenderReport,
    },
    input::{
        Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector, EventMeta,
        PhysicalControlId, PhysicalInputEvent, VendorNamespaceId,
    },
    judge::{JudgeEngine, JudgeGrade, JudgeOutcome, JudgeProfile, JudgeWindow},
    runtime::{Runtime, RuntimeReport},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use beatkernel_platform::audio::{encode_pcm, DeviceFormat, SampleEncoding};
use std::{collections::BTreeMap, error::Error, fmt, io::Write};

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
fn accept(report: RuntimeReport, summary: &mut OfflineReport) -> Result<(), OfflineError> {
    summary.judge_results += report.judge_events.len();
    summary.hits += report
        .judge_events
        .iter()
        .filter(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
        .count();
    if let Some(error) = report.judge_error {
        return Err(failure(error, summary.last_render));
    }
    if !report.audio_failures.is_empty() {
        return Err(OfflineError {
            message: "audio command admission failed".into(),
            last_render: summary.last_render,
            audio_failures: report.audio_failures,
        });
    }
    Ok(())
}
fn render_block(
    mixer: &mut Mixer,
    pcm: &mut [f32],
    bytes: &mut [u8],
    format: DeviceFormat,
    output: &mut dyn Write,
    summary: &mut OfflineReport,
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
        || c.unknown_stops != 0
        || c.invalid_gains != 0
        || c.invalid_rates != 0
        || c.invalid_times != 0
    {
        return Err(failure(
            format!("core audio execution rejected commands: {:?}", c),
            Some(report),
        ));
    }
    encode_pcm(format, pcm, bytes).map_err(|error| failure(error, Some(report)))?;
    output
        .write_all(bytes)
        .map_err(|error| failure(error, Some(report)))?;
    summary.frames += report.frames as u64;
    Ok(())
}

/// Renders only admitted target frames through the actual Runtime/JudgeEngine/Mixer.
///
/// Synthetic input preserves exact compiled times; overlapping lane/hold patterns
/// retain real judge outcomes. Same-frame capacity overflow is explicit. The sink
/// is not flushed; any failure may leave a prefix. No native playback occurs.
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
        .ok_or("offline schedule extent overflow")?;
    let mut records = Vec::new();
    records.try_reserve_exact(schedule_len)?;
    let mut lanes = BTreeMap::new();
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
            matches!(record.item, Item::Input { .. }),
            record.ordinal,
        )
    });
    let bindings =
        BindingMap::from_bindings(lanes.into_iter().map(|(channel, game_control)| Binding {
            device: DeviceSelector::Exact(DeviceId(1)),
            physical: physical(channel),
            game_control,
        }))?;
    let judge = JudgeEngine::new(
        prepared.compiled.chart,
        prepared.source.rules(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::from_nanos(1_000_000),
                late: Duration::from_nanos(1_000_000),
            }],
            Duration::ZERO,
        )?,
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
    while summary.frames < options.frames {
        let next_frame = records
            .get(index)
            .map_or(options.frames, |record| record.frame.min(options.frames));
        if next_frame > summary.frames {
            let frames =
                usize::try_from((next_frame - summary.frames).min(options.block_frames as u64))?;
            render_block(
                &mut mixer,
                &mut pcm[..frames * channels],
                &mut bytes[..frames * channels * 4],
                encoded,
                output,
                &mut summary,
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
                        accept(report, &mut summary)?;
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
        accept(report, &mut summary)?;
    }
    Ok(summary)
}
