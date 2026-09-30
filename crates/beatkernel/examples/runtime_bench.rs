//! Portable CPU workload through the actual runtime, scalar queue and PCM mixer.
use beatkernel::{
    audio::{
        command_queue, AudioCounters, AudioFormat, AudioLimits, Mixer, MixerConfig, PcmLimits,
        PcmSample, SampleBank, SampleId, VoiceId,
    },
    chart::{
        Beat, Bpm, InteractionId, ObjectId, ObjectMetadata, SourceChart, SourceObject, VisualId,
    },
    input::{
        BackendId, Binding, BindingMap, ButtonEvent, ButtonState, DeviceId, DeviceSelector,
        EventMeta, GameControlId, NativeEventMeta, PhysicalControlId, PhysicalInputEvent,
    },
    interaction::InstantEvaluator,
    judge::{JudgeEngine, JudgeGrade, JudgeProfile, JudgeStage, JudgeWindow, Rule},
    runtime::{Runtime, SoundBinding},
    telemetry::{IntervalJitter, RuntimeCounters, RuntimeTelemetry},
    time::{ClockDomainId, ClockMapper, ClockMappingQuality, ClockPoint, Duration, Timestamp},
    transport::{Rate, Transport},
};
use std::{error::Error, hint::black_box, time::Instant};

const HOST_ORIGIN: i64 = 1_000_000_000;
const HELP: &str = "Portable BeatKernel runtime CPU benchmark\nUsage: cargo run --release -p beatkernel --example runtime_bench -- [options]\n  --iterations N         measured blocks, 1..10000 (default 2000)\n  --warmup N             warmup blocks, 0..1000 (default 100)\n  --buffer-frames N      maximum variable render frames, 8..4096 (default 128)\n  --queue-capacity N     scalar queue slots, 1..65536 (default 64)\n  --telemetry-samples N  retained observations per timing ring, 1..65536 (default 4096)\n  --sample-rate N        frames/second, 8000..192000 (default 48000)\n  --help\nOffline PCM rendering and explicitly generated synthetic interval jitter only. Physical latency, native jitter and actual underruns are unavailable. No hardware or optimization claim.";

#[derive(Clone, Copy, Debug)]
struct Options {
    iterations: usize,
    warmup: usize,
    buffer_frames: usize,
    queue_capacity: usize,
    telemetry_samples: usize,
    sample_rate: u32,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            iterations: 2000,
            warmup: 100,
            buffer_frames: 128,
            queue_capacity: 64,
            telemetry_samples: 4096,
            sample_rate: 48_000,
        }
    }
}
fn parse(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut options = Options::default();
    if !args.len().is_multiple_of(2) {
        return Err("each option requires one integer value; use --help".into());
    }
    for pair in args.chunks_exact(2) {
        let value = pair[1]
            .parse::<usize>()
            .map_err(|_| format!("{} requires an unsigned integer", pair[0]))?;
        match pair[0].as_str() {
            "--iterations" => options.iterations = value,
            "--warmup" => options.warmup = value,
            "--buffer-frames" => options.buffer_frames = value,
            "--queue-capacity" => options.queue_capacity = value,
            "--telemetry-samples" => options.telemetry_samples = value,
            "--sample-rate" => options.sample_rate = u32::try_from(value)?,
            _ => return Err(format!("unknown option {}; use --help", pair[0]).into()),
        }
    }
    if !(1..=10000).contains(&options.iterations)
        || options.warmup > 1000
        || !(8..=4096).contains(&options.buffer_frames)
        || !(1..=65536).contains(&options.queue_capacity)
        || !(1..=65536).contains(&options.telemetry_samples)
        || !(8000..=192000).contains(&options.sample_rate)
    {
        return Err("option outside documented bounds; use --help".into());
    }
    Ok(options)
}

struct FixtureClocks;
impl ClockMapper for FixtureClocks {
    fn map(&self, point: ClockPoint, to: ClockDomainId) -> Option<Timestamp> {
        match (point.domain.0, to.0) {
            (1, 2) => point
                .timestamp
                .checked_add(Duration::from_nanos(HOST_ORIGIN)),
            (2, 3) => point
                .timestamp
                .checked_sub(Duration::from_nanos(HOST_ORIGIN)),
            _ => None,
        }
    }
    fn quality(&self) -> ClockMappingQuality {
        ClockMappingQuality::Exact
    }
}
#[derive(Clone, Copy)]
struct Block {
    frames: usize,
    note_frame: u64,
    input_frame: u64,
    control: u32,
    sequence: u64,
}
struct Workload {
    runtime: Runtime,
    mixer: Mixer,
    blocks: Vec<Block>,
    output: Vec<f32>,
}
fn frame_time(frame: u64, rate: u32) -> Result<Timestamp, Box<dyn Error>> {
    let nanos = (u128::from(frame) * 1_000_000_000) / u128::from(rate);
    Ok(Timestamp::from_nanos(i64::try_from(nanos)?))
}
fn prepare(options: Options) -> Result<Workload, Box<dyn Error>> {
    let count = options
        .iterations
        .checked_add(options.warmup)
        .ok_or("iteration overflow")?;
    let mut chart = SourceChart::new(1_000_000_000, Bpm::new(60, 1)?)?;
    let mut blocks = Vec::with_capacity(count);
    let mut sounds = Vec::with_capacity(count);
    let mut start = 0u64;
    for index in 0..count {
        let frames = match index % 4 {
            0 => options.buffer_frames,
            1 => options.buffer_frames / 2,
            2 => options.buffer_frames * 3 / 4,
            _ => options.buffer_frames,
        };
        let note_frame = start
            .checked_add((frames / 2) as u64)
            .ok_or("frame overflow")?;
        // Each seventeenth event is intentionally three frames late; remaining
        // events vary by +/- one frame inside the two-frame acceptance window.
        let input_frame = if index % 17 == 16 {
            note_frame.checked_add(3).ok_or("frame overflow")?
        } else {
            match index % 3 {
                0 => note_frame.checked_sub(1).ok_or("frame underflow")?,
                1 => note_frame,
                _ => note_frame.checked_add(1).ok_or("frame overflow")?,
            }
        };
        let control = (index % 2 + 1) as u32;
        let id = ObjectId(index as u64 + 1);
        chart.objects.push(SourceObject {
            id,
            start: Beat::new(frame_time(note_frame, options.sample_rate)?.as_nanos())?,
            end: None,
            interaction: InteractionId(control),
            visual: VisualId(control),
            audio: None,
            metadata: ObjectMetadata::default(),
        });
        sounds.push(SoundBinding {
            object: id,
            stage: JudgeStage::Instant,
            sample: SampleId(control as u64),
            voice: VoiceId((index % 8 + 1) as u64),
            gain: [0.25, 0.5, 0.75, 1.0][index % 4],
        });
        blocks.push(Block {
            frames,
            note_frame,
            input_frame,
            control,
            sequence: (index as u64).checked_mul(4).ok_or("sequence overflow")?,
        });
        start = start.checked_add(frames as u64).ok_or("frame overflow")?;
    }
    let window = Duration::from_nanos(frame_time(2, options.sample_rate)?.as_nanos());
    let judge = JudgeEngine::new(
        chart.compile()?,
        (1..=2)
            .map(|control| Rule {
                interaction: InteractionId(control),
                control: GameControlId(control),
                evaluator: Box::new(InstantEvaluator),
            })
            .collect(),
        JudgeProfile::new(
            vec![JudgeWindow {
                grade: JudgeGrade(1),
                early: window,
                late: window,
            }],
            Duration::ZERO,
        )?,
    )?;
    let bindings = BindingMap::from_bindings((1..=2).map(|control| Binding {
        device: DeviceSelector::Exact(DeviceId(control as u64)),
        physical: PhysicalControlId::keyboard(control as u16 + 3),
        game_control: GameControlId(control),
    }))?;
    let (producer, consumer) = command_queue(options.queue_capacity)?;
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
        sounds,
        options.telemetry_samples,
    )?;
    let format = AudioFormat::new(options.sample_rate, 2)?;
    let pcm_limits = PcmLimits::new(65536, 131072, 2)?;
    let mut bank = SampleBank::new(format, pcm_limits)?;
    for control in 1..=2 {
        let pcm: Vec<f32> = (0..256)
            .map(|sample| (((sample + control * 3) % 32) as f32 / 31.0 - 0.5) * 0.1)
            .collect();
        bank.insert(
            SampleId(control as u64),
            PcmSample::new(format, pcm, pcm_limits)?,
        )?;
    }
    let mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(3),
            Timestamp::ZERO,
            AudioLimits::new(
                options.queue_capacity,
                8,
                options.queue_capacity,
                options.buffer_frames,
                options.queue_capacity,
            )?,
        ),
        bank,
        consumer,
    )?;
    Ok(Workload {
        runtime,
        mixer,
        blocks,
        output: vec![0.0; options.buffer_frames * 2],
    })
}

impl Workload {
    fn block(
        &mut self,
        block: Block,
        rate: u32,
        input_timing: &mut RuntimeTelemetry,
        mixer_timing: &mut RuntimeTelemetry,
    ) -> Result<f64, Box<dyn Error>> {
        let input_at = frame_time(block.input_frame, rate)?;
        let audio_at = frame_time(block.note_frame, rate)?
            .checked_add(Duration::from_nanos(HOST_ORIGIN))
            .ok_or("audio timestamp overflow")?;
        let mut emit = |source, control: u16, sequence, state| -> Result<(), Box<dyn Error>> {
            let point = ClockPoint {
                domain: ClockDomainId(1),
                timestamp: input_at,
            };
            let mut meta = EventMeta::new(DeviceId(source), point, sequence);
            meta.native = Some(NativeEventMeta {
                backend: BackendId(90),
                code: Some(u32::from(control)),
                timestamp: Some(point),
            });
            let input_started = Instant::now();
            let report = self.runtime.process_input(
                PhysicalInputEvent::Button(ButtonEvent {
                    meta,
                    control: PhysicalControlId::keyboard(control),
                    state,
                }),
                &FixtureClocks,
                ClockPoint {
                    domain: ClockDomainId(2),
                    timestamp: audio_at,
                },
            )?;
            input_timing.record_processing_ns(u64::try_from(input_started.elapsed().as_nanos())?);
            if let Some(error) = report.judge_error {
                return Err(error.into());
            }
            black_box(report);
            Ok(())
        };
        emit(
            block.control as u64,
            block.control as u16 + 3,
            block.sequence,
            ButtonState::Down,
        )?;
        if block.sequence % 16 == 0 {
            emit(
                block.control as u64,
                block.control as u16 + 3,
                block.sequence + 1,
                ButtonState::Repeat,
            )?;
        }
        emit(
            block.control as u64,
            block.control as u16 + 3,
            block.sequence + 2,
            ButtonState::Up,
        )?;
        if block.sequence % 28 == 0 {
            emit(9, 6, block.sequence + 3, ButtonState::Down)?;
        }
        let started = Instant::now();
        let report = self.mixer.render(&mut self.output[..block.frames * 2])?;
        let nanos = u64::try_from(started.elapsed().as_nanos())?;
        mixer_timing.record_processing_ns(nanos);
        black_box(report);
        Ok(self.output[..block.frames * 2]
            .iter()
            .map(|value| f64::from(*value))
            .sum())
    }
}
fn difference(after: u64, before: u64) -> u64 {
    after.saturating_sub(before)
}
fn runtime_delta(after: RuntimeCounters, before: RuntimeCounters) -> RuntimeCounters {
    RuntimeCounters {
        inputs: difference(after.inputs, before.inputs),
        unbound: difference(after.unbound, before.unbound),
        rejected: difference(after.rejected, before.rejected),
        judge_results: difference(after.judge_results, before.judge_results),
        audio_commands: difference(after.audio_commands, before.audio_commands),
        queue_full: difference(after.queue_full, before.queue_full),
        queue_disconnected: difference(after.queue_disconnected, before.queue_disconnected),
        input_drops: difference(after.input_drops, before.input_drops),
        audio_underruns: difference(after.audio_underruns, before.audio_underruns),
    }
}
fn audio_delta(after: AudioCounters, before: AudioCounters) -> AudioCounters {
    AudioCounters {
        rendered_frames: difference(after.rendered_frames, before.rendered_frames),
        commands_consumed: difference(after.commands_consumed, before.commands_consumed),
        commands_applied: difference(after.commands_applied, before.commands_applied),
        late_commands: difference(after.late_commands, before.late_commands),
        pending_full: difference(after.pending_full, before.pending_full),
        voice_full: difference(after.voice_full, before.voice_full),
        unknown_samples: difference(after.unknown_samples, before.unknown_samples),
        unknown_stops: difference(after.unknown_stops, before.unknown_stops),
        invalid_gains: difference(after.invalid_gains, before.invalid_gains),
        invalid_rates: difference(after.invalid_rates, before.invalid_rates),
        invalid_times: difference(after.invalid_times, before.invalid_times),
    }
}
fn run(options: Options) -> Result<(), Box<dyn Error>> {
    let mut workload = prepare(options)?;
    let mut input_timing = RuntimeTelemetry::new(options.telemetry_samples);
    let mut mixer_timing = RuntimeTelemetry::new(options.telemetry_samples);
    for index in 0..options.warmup {
        let block = workload.blocks[index];
        black_box(workload.block(
            block,
            options.sample_rate,
            &mut input_timing,
            &mut mixer_timing,
        )?);
    }
    let baseline_runtime = workload.runtime.telemetry().counters();
    let baseline_audio = workload.mixer.counters();
    // No core reset API is required: measurement retains the operation durations
    // independently and takes counter deltas from warmed gameplay/audio owners.
    input_timing = RuntimeTelemetry::new(options.telemetry_samples);
    mixer_timing = RuntimeTelemetry::new(options.telemetry_samples);
    // Generated timestamps have a separate synthetic domain and explicit
    // nominal cadence. They are independent of CPU time and native callbacks.
    let nominal = Duration::from_nanos(1_000_000);
    let mut synthetic_at = ClockPoint {
        domain: ClockDomainId(90),
        timestamp: Timestamp::ZERO,
    };
    let mut synthetic_jitter =
        IntervalJitter::new(options.telemetry_samples, nominal, synthetic_at)?;
    let started = Instant::now();
    let mut checksum = 0.0;
    for index in options.warmup..workload.blocks.len() {
        let block = workload.blocks[index];
        let deviation = [-20_000, 10_000, 0, -5_000, 2_000][(index - options.warmup) % 5];
        let interval = nominal
            .checked_add(Duration::from_nanos(deviation))
            .ok_or("synthetic interval overflow")?;
        synthetic_at.timestamp = synthetic_at
            .timestamp
            .checked_add(interval)
            .ok_or("synthetic clock overflow")?;
        black_box(synthetic_jitter.observe(synthetic_at)?);
        checksum += workload.block(
            block,
            options.sample_rate,
            &mut input_timing,
            &mut mixer_timing,
        )?;
    }
    let elapsed = started.elapsed();
    let elapsed_ns = elapsed.as_nanos();
    let runtime = runtime_delta(workload.runtime.telemetry().counters(), baseline_runtime);
    let audio = audio_delta(workload.mixer.counters(), baseline_audio);
    println!("software-only CPU workload; settings={options:?}; elapsed_ns={elapsed_ns}");
    println!(
        "measured_blocks={} software_operations={} rendered_frames={} pcm_checksum={}",
        options.iterations,
        runtime.inputs,
        audio.rendered_frames,
        black_box(checksum)
    );
    if elapsed_ns != 0 {
        println!(
            "software_operations_per_second={:.2}",
            runtime.inputs as f64 * 1_000_000_000.0 / elapsed_ns as f64
        );
    }
    println!(
        "runtime_input_call_execution_ns={:?} capacity={} (measured process_input calls only)",
        input_timing.processing(),
        options.telemetry_samples
    );
    println!(
        "mixer_render_execution_ns={:?} capacity={} (offline varying buffer sizes)",
        mixer_timing.processing(),
        options.telemetry_samples
    );
    println!("synthetic_generated_interval_jitter_ns={:?} nominal_ns={} clock_domain={} pairs={} (generated timestamps; not measured callback/device timing)", synthetic_jitter.summary()?, nominal.as_nanos(), synthetic_at.domain.0, synthetic_jitter.observed_pairs());
    println!("software_runtime_counters={runtime:?}");
    println!("software_mixer_counters={audio:?}");
    println!("native_input_latency=unavailable physical_output_latency=unavailable native_callback_arrival_jitter=unavailable actual_native_underruns=unavailable");
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("{HELP}");
        return Ok(());
    }
    run(parse(&args)?)
}
