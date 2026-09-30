//! Offline recorded-sound rendering through the shared planner and actual Mixer.
use crate::{
    offline::{render_block, OfflineError, OfflineOptions, OfflineReport},
    replay_audio::plan_audio,
    PreparedBms,
};
use beatkernel::{
    audio::{command_queue, AudioFormat, AudioLimits, Mixer, MixerConfig, RenderReport},
    judge::JudgeOutcome,
    replay::codec::{ReplayCodecLimits, ReplayFile},
    time::{ClockDomainId, ClockPoint, Duration, Timestamp},
};
use beatkernel_platform::audio::{DeviceFormat, SampleEncoding};
use std::{error::Error, io::Write};

/// Logical replay evidence and independently bounded PCM output.
#[derive(Clone, Copy, Debug)]
pub struct ReplayRenderReport {
    /// Complete frames written to the sink.
    pub frames: u64,
    /// Interleaved float32 output format.
    pub format: AudioFormat,
    /// Commands admitted within the requested output extent.
    pub commands_admitted: usize,
    /// Actual JudgeEvents from the full recording, independently of output cutoff.
    pub judge_results: usize,
    /// Hit stages from the full recording.
    pub hits: usize,
    /// Last original recorded song time, absent for an empty log.
    pub recorded_until: Option<Timestamp>,
    /// Actual final logical judge hash for the full recording.
    pub final_judge_hash: u64,
    /// Latest completed core render; no native delivery is implied.
    pub last_render: Option<RenderReport>,
}

fn target_frame(at: Timestamp, origin: Timestamp, rate: u32) -> Result<u64, Box<dyn Error>> {
    let elapsed = i128::from(at.as_nanos()) - i128::from(origin.as_nanos());
    if elapsed < 0 {
        return Err("replay audio command precedes output origin".into());
    }
    let scaled = elapsed
        .checked_mul(i128::from(rate))
        .ok_or("replay audio frame arithmetic overflow")?;
    Ok(u64::try_from(
        scaled
            .checked_add(999_999_999)
            .ok_or("replay audio frame ceiling overflow")?
            / 1_000_000_000,
    )?)
}

/// Validates the full logical replay and renders its selected song-time sounds.
///
/// Commands at/beyond the output extent are excluded; judge counts/hash still
/// describe the full recording. Only one target-frame group is admitted at a time.
/// Actual core execution and writer errors retain shared OfflineError evidence.
/// Zero frames validates without admission/output; the sink is never flushed.
/// This cannot reproduce original native scheduling or dropped audio admissions.
pub fn render_replay(
    prepared: PreparedBms,
    file: ReplayFile,
    limits: ReplayCodecLimits,
    options: OfflineOptions,
    preroll: Duration,
    output: &mut dyn Write,
) -> Result<ReplayRenderReport, Box<dyn Error>> {
    let format = prepared.bank.format();
    let rate = format.sample_rate();
    let channels = usize::from(format.channels());
    let audio_limits = AudioLimits::new(
        options.command_capacity,
        options.max_voices,
        options.command_capacity,
        options.block_frames,
        options.command_capacity,
    )?;
    let extent_ns =
        (i128::from(options.frames) * 1_000_000_000 + i128::from(rate) - 1) / i128::from(rate);
    i64::try_from(extent_ns)
        .map_err(|_| "replay output duration exceeds representable timestamps")?;
    options
        .frames
        .checked_mul(u64::from(format.channels()))
        .and_then(|samples| samples.checked_mul(4))
        .ok_or("replay output byte extent overflow")?;
    let encoded = DeviceFormat::new(rate, format.channels(), SampleEncoding::Float32, None)?;
    let samples = options
        .block_frames
        .checked_mul(channels)
        .ok_or("replay render block extent overflow")?;
    let byte_count = samples
        .checked_mul(4)
        .ok_or("replay render block byte extent overflow")?;
    let mut pcm = Vec::new();
    pcm.try_reserve_exact(samples)?;
    pcm.resize(samples, 0.0);
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(byte_count)?;
    bytes.resize(byte_count, 0);
    let origin = ClockPoint {
        domain: ClockDomainId(0x424d53),
        timestamp: Timestamp::ZERO,
    };
    let plan = plan_audio(&prepared, file, limits, origin, preroll)?;
    // Check every target before the first write, including commands excluded by
    // the caller's output cutoff. Reuse the immutable plan rather than copying it.
    let mut previous = None;
    for command in &plan.commands {
        let frame = target_frame(command.at(), origin.timestamp, rate)?;
        if previous.is_some_and(|previous| frame < previous) {
            return Err("replay audio plan command chronology regressed".into());
        }
        previous = Some(frame);
    }
    let judge_results = plan.judge_events.len();
    let hits = plan
        .judge_events
        .iter()
        .filter(|event| matches!(event.outcome, JudgeOutcome::Hit { .. }))
        .count();
    let mut summary = OfflineReport {
        frames: 0,
        format,
        hits,
        judge_results,
        last_render: None,
    };
    let (mut producer, consumer) = command_queue(options.command_capacity)?;
    let mut mixer = Mixer::new(
        MixerConfig::new(format, origin.domain, origin.timestamp, audio_limits),
        prepared.bank,
        consumer,
    )?;
    let mut index = 0;
    while summary.frames < options.frames {
        let next_frame = match plan.commands.get(index) {
            Some(command) => {
                target_frame(command.at(), origin.timestamp, rate)?.min(options.frames)
            }
            None => options.frames,
        };
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
            while let Some(command) = plan.commands.get(index).copied() {
                if target_frame(command.at(), origin.timestamp, rate)? != summary.frames {
                    break;
                }
                producer.try_push(command).map_err(|error| OfflineError {
                    message: "replay audio command admission failed".into(),
                    last_render: summary.last_render,
                    audio_failures: vec![error],
                })?;
                index += 1;
            }
        }
    }
    Ok(ReplayRenderReport {
        frames: summary.frames,
        format,
        commands_admitted: index,
        judge_results,
        hits,
        recorded_until: plan.recorded_until,
        final_judge_hash: plan.final_judge_hash,
        last_render: summary.last_render,
    })
}
