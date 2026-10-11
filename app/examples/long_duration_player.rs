//! Opt-in whole-timeline software traversal through the original player ports.
#[path = "long_duration_player/workload.rs"]
mod workload;

use beatkernel::judge::JudgeStage;
use beatkernel_bms_runtime::native_gameplay_host::PauseState;
use std::error::Error;
use workload::{Options, Report};

const HELP: &str = "BeatKernel continuous long-duration virtual player traversal
Usage: cargo run --release -p beatkernel-bms-runtime --no-default-features --example long_duration_player -- [options]
  --hours N         full virtual playback duration, 1..168 (default 20)
  --block-frames N  maximum render block, 128..65536 (default 8192)
  --help            show this help; use alone
Each flag may appear once and requires one representable decimal integer.
Combined allowance: 2 * ceil(hours * 3600 * 1000 / block-frames) + 8 * hours + 64
must not exceed 200000 render calls plus replay operations.
Every successive sample from song zero is rendered and checked, including idle
intervals and a genuine pause/resume. Fixed 1000 Hz mono with sparse hourly BMS
notes is a software correctness grid, not standard-rate or hardware performance.
Physical devices, native latency, GPU, network and wall-clock soak are unavailable.
Hashes use FNV-1a-64: PCM f32 bit patterns in little-endian byte order and canonical
encoded replay bytes. They identify observations, not cryptographic proof.";

fn parse(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut options = Options::default();
    let mut seen = [false; 2];
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--hours" => 0,
            "--block-frames" => 1,
            "--help" => return Err("--help must be used alone".into()),
            _ => return Err(format!("unknown option {flag}; use --help").into()),
        };
        if seen[slot] {
            return Err(format!("duplicate option {flag}; use --help").into());
        }
        seen[slot] = true;
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires one integer value; use --help"))?;
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(
                format!("{flag} requires a representable decimal integer; use --help").into(),
            );
        }
        let value: usize = value
            .parse()
            .map_err(|_| format!("{flag} requires a representable decimal integer; use --help"))?;
        match slot {
            0 => options.hours = value,
            1 => options.block_frames = value,
            _ => unreachable!(),
        }
        index += 2;
    }
    // Admission precedes any playback-owner allocation in workload::run.
    options.validate()?;
    Ok(options)
}

fn stage_counts(report: &Report) -> [usize; 4] {
    let mut counts = [0; 4];
    for event in &report.live_results {
        counts[match event.stage {
            JudgeStage::Instant => 0,
            JudgeStage::HoldHead => 1,
            JudgeStage::HoldTail => 2,
            JudgeStage::Custom(_) => 3,
        }] += 1;
    }
    counts
}

fn verify_summary(report: &Report) -> Result<(), Box<dyn Error>> {
    let hours = report.options.hours;
    let horizon = hours as u64 * 3_600_000;
    if !report.completed
        || report.playback_frames != horizon
        || !(horizon + 3..=horizon + 8).contains(&report.physical_frames)
        || report.verified_samples != report.physical_frames
        || report.section_end.as_nanos() != horizon as i64 * 1_000_000
        || report.input_edges != 2 * hours
        || report.key_samples != hours + hours / 2
        || report.bgm_samples != hours
        || stage_counts(report) != [hours.div_ceil(2), hours / 2, hours / 2, 0]
        || report.live_results != report.reconstructed_results
        || report.record_operations != report.file.records.len()
        || report.repair_records != 0
        || report.render_calls + report.record_operations > 200_000
        || report.cached_report_reads <= report.render_calls
        || report.checkpoints.is_empty()
        || report.checkpoints.len() > 512
        || report.pause_render_count != 3
        || !report.pause_states.contains(&PauseState::Paused)
        || !report.pause_states.contains(&PauseState::Running)
        || report.pending_inputs != 0
        || report.pending_commands != 0
        || report.active_voices != 0
        || report.mixer_identity_before == 0
        || report.mixer_identity_before != report.mixer_identity_after
    {
        return Err(
            "whole-timeline summary coverage, judge, pause, owner or drain invariant failed".into(),
        );
    }
    Ok(())
}

fn summary(report: &Report) {
    let [instant, hold_head, hold_tail, custom] = stage_counts(report);
    println!(
        "traversal_schema=1 workload_id=beatkernel-long-duration-player-v1 debug_assertions={}",
        cfg!(debug_assertions)
    );
    println!(
        "hours={} block_frames={} sample_rate_hz=1000 channels=1 bpm=60 measure_ratio=900 chart_objects={} bgm_cues={}",
        report.options.hours,
        report.options.block_frames,
        report.options.hours,
        report.options.hours
    );
    println!(
        "song_start_ns=0 section_end_ns={} horizon_frames={} playback_frames={} physical_frames={} verified_samples={} every_sample_verified=true completed={}",
        report.section_end.as_nanos(),
        report.options.hours as u64 * 3_600_000,
        report.playback_frames,
        report.physical_frames,
        report.verified_samples,
        report.completed
    );
    println!(
        "render_calls={} record_operations={} combined_operations={} operation_limit=200000 cached_report_reads={} original_input_edges={} advance_records={} repair_records={}",
        report.render_calls,
        report.record_operations,
        report.render_calls + report.record_operations,
        report.cached_report_reads,
        report.input_edges,
        report.advance_records,
        report.repair_records
    );
    println!(
        "live_judges={} reconstructed_judges={} instant_judges={} hold_head_judges={} hold_tail_judges={} custom_judges={} live_replay_equal=true key_samples={} bgm_samples={}",
        report.live_results.len(),
        report.reconstructed_results.len(),
        instant,
        hold_head,
        hold_tail,
        custom,
        report.key_samples,
        report.bgm_samples
    );
    println!(
        "pcm_hash={} canonical_replay_hash={} replay_bytes={} hash_algorithm=fnv1a64 pcm_encoding=f32-bits-little-endian replay_encoding=canonical-replay-bytes",
        report.pcm_hash,
        report.replay_hash,
        report.bytes.len()
    );
    println!(
        "pause_states={:?} pause_render_count={} retained_mixer=true pending_inputs={} pending_commands={} active_voices={} checkpoints={} checkpoint_limit=512",
        report.pause_states,
        report.pause_render_count,
        report.pending_inputs,
        report.pending_commands,
        report.active_voices,
        report.checkpoints.len()
    );
    for (index, checkpoint) in report.checkpoints.iter().enumerate() {
        println!(
            "checkpoint={} physical_frame={} playback_frame={} paused={}",
            index, checkpoint.physical_frame, checkpoint.playback_frame, checkpoint.paused
        );
    }
    println!(
        "software_only=true source_grid=sparse-1000hz standard_rate_performance=unavailable physical_devices=unavailable native_latency=unavailable gpu=unavailable network=unavailable wall_clock_soak=unavailable"
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("{HELP}");
        return Ok(());
    }
    let report = workload::run(parse(&args)?)?;
    verify_summary(&report)?;
    summary(&report);
    Ok(())
}
