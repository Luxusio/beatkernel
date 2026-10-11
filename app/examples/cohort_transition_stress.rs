//! Opt-in, bounded software stress through the original retained player owners.
#[path = "cohort_transition_stress/workload.rs"]
mod workload;

use beatkernel_bms_runtime::native_gameplay_host::PauseState;
use std::error::Error;
use workload::{Options, Report};

const HELP: &str = "BeatKernel retained solo/cohort transition software stress
Usage: cargo run --release -p beatkernel-bms-runtime --no-default-features --example cohort_transition_stress -- [options]
  --members N           original local players, 1..64 (default 2)
  --cycles N            completed attempts, 1..64 (default 8)
  --notes-per-member N  chart objects per attempt/player, 1..64 (default 8)
  --help                show this help; use alone
Each flag may appear once and requires one representable integer value.
members * cycles * notes-per-member must not exceed 262144.
Cycles include the initial attempt; attempt ordinals start at zero.
Actual production solo/cohort pumps, Mixer PCM and replay captures use virtual
input/output ports. Successful output replacement is a separate admitted
composition; replacement during retained practice is explicitly refused.
Physical devices, native latency, GPU, network and wall-clock soak are unavailable.
Hashes are deterministic FNV-1a-64 identity observations, not cryptographic proof.";

fn parse(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut options = Options::default();
    let mut seen = [false; 3];
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--members" => 0,
            "--cycles" => 1,
            "--notes-per-member" => 2,
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
        let value: usize = value
            .parse()
            .map_err(|_| format!("{flag} requires a representable integer; use --help"))?;
        match slot {
            0 => options.members = value,
            1 => options.cycles = value,
            2 => options.notes_per_member = value,
            _ => unreachable!(),
        }
        index += 2;
    }
    options.validate()?;
    Ok(options)
}

/// FNV-1a over literal bytes; PCM uses each f32 bit pattern in little-endian order.
fn hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |value, byte| {
        (value ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn verify_summary(report: &Report) -> Result<(), Box<dyn Error>> {
    let options = report.options;
    let generations: Vec<_> = (1..=options.cycles as u64).collect();
    let transitions: Vec<_> = (2..=options.cycles as u64).collect();
    if report.generations != generations
        || report.applied_generations != transitions
        || report.committed_generations != transitions
        || report.attempt_starts.len() != options.cycles
        || report.captures.len() != options.members * options.cycles
        || report.transitions != options.cycles - 1
        || report.authored_edges != 2 * options.members * options.cycles * options.notes_per_member
        || report.captured_input_edges != report.authored_edges
        || report.advance_records == 0
        || report.repair_records != 0
        || report.pending_inputs != 0
        || report.pending_commands != 0
        || report.active_voices != 0
        || report.mixer_identity_before == 0
        || report.mixer_identity_before != report.mixer_identity_after
        || report.sentinel_injected != (options.cycles > 1)
        || report
            .pause_states
            .iter()
            .filter(|state| **state == PauseState::Paused)
            .count()
            < options.cycles
        || report
            .pause_states
            .iter()
            .filter(|state| **state == PauseState::Running)
            .count()
            < options.cycles
    {
        return Err("retained summary lineage, counts, pause or drain invariant failed".into());
    }
    for (index, start) in report.attempt_starts.iter().enumerate() {
        if start.attempt != index
            || start.generation != index as u64 + 1
            || start.song_start.as_nanos() != 0
            || (index > 0
                && (start.physical_frame <= report.attempt_starts[index - 1].physical_frame
                    || start.playback_frame <= report.attempt_starts[index - 1].playback_frame))
        {
            return Err(
                "original attempt association or advancing frame grid invariant failed".into(),
            );
        }
    }
    // At most 4096 cold capture associations; no per-frame summary allocation.
    let mut seen = vec![false; options.members * options.cycles];
    for capture in &report.captures {
        let member = capture.player.0 as usize;
        if member == 0
            || member > options.members
            || capture.attempt >= options.cycles
            || capture.device.0 != 1000 + member as u64
            || capture.bytes.is_empty()
            || capture.live_results != capture.reconstructed_results
            || capture.reconstructed_results.len()
                != options.notes_per_member + options.notes_per_member / 2
        {
            return Err(
                "original member capture association or live/replay invariant failed".into(),
            );
        }
        let slot = capture.attempt * options.members + member - 1;
        if std::mem::replace(&mut seen[slot], true) {
            return Err("duplicate original member capture association".into());
        }
        let expected_path = if capture.attempt == 0 {
            format!("member{member}.bkr")
        } else {
            format!("member{member}.retry{}.bkr", capture.attempt)
        };
        if capture.path.file_name().and_then(|name| name.to_str()) != Some(expected_path.as_str()) {
            return Err("original recording attempt path invariant failed".into());
        }
    }
    let replacement = &report.replacement;
    if replacement.mixer_identity_before == 0
        || replacement.mixer_identity_before != replacement.mixer_identity_after
        || replacement.epoch_before != 0
        || replacement.epoch_after != 1
        || replacement.retire_calls != 1
        || replacement.open_calls != 1
        || !replacement.premature_recovery_refused
        || replacement.snapshots.len() != 2
        || replacement.paused_render_reports.len() < 3
        || replacement
            .paused_render_reports
            .iter()
            .any(|render| !render.paused || render.frames == 0 || render.playback_frames != 0)
        || report.replacement_refusal
            != "output replacement is unavailable during retained practice"
        || report.refusal_mapping_calls != 0
        || report.refusal_retire_calls != 0
    {
        return Err(
            "separate admitted replacement or retained-practice refusal invariant failed".into(),
        );
    }
    Ok(())
}

fn summary(report: &Report) {
    println!(
        "stress_schema=1 workload_id=beatkernel-cohort-transition-v1 debug_assertions={}",
        cfg!(debug_assertions)
    );
    println!(
        "members={} cycles={} notes_per_member={} captures={} transitions={}",
        report.options.members,
        report.options.cycles,
        report.options.notes_per_member,
        report.captures.len(),
        report.transitions
    );
    println!(
        "authored_edges={} captured_input_edges={} advance_records={} repair_records={}",
        report.authored_edges,
        report.captured_input_edges,
        report.advance_records,
        report.repair_records
    );
    println!(
        "generations={:?} applied_generations={:?} committed_generations={:?}",
        report.generations, report.applied_generations, report.committed_generations
    );
    for start in &report.attempt_starts {
        println!(
            "attempt={} generation={} physical_frame={} playback_frame={} song_start_ns={}",
            start.attempt,
            start.generation,
            start.physical_frame,
            start.playback_frame,
            start.song_start.as_nanos()
        );
    }
    for capture in &report.captures {
        println!("capture_attempt={} player={} device={} path={} replay_bytes={} replay_hash={} judge_hash={} live_results={} reconstructed_results={}",
            capture.attempt, capture.player.0, capture.device.0, capture.path.display(),
            capture.bytes.len(), hash(capture.bytes.iter().copied()), capture.judge_hash,
            capture.live_results.len(), capture.reconstructed_results.len());
    }
    println!("pcm_samples={} pcm_hash={} render_reports={} hash_algorithm=fnv1a64 pcm_encoding=f32-bits-little-endian",
        report.pcm.len(), hash(report.pcm.iter().flat_map(|sample| sample.to_bits().to_le_bytes())), report.render_reports.len());
    println!("pause_states={:?} retained_mixer=true sentinel_injected={} sentinel_absent=true pending_inputs={} pending_commands={} active_voices={}",
        report.pause_states, report.sentinel_injected, report.pending_inputs, report.pending_commands, report.active_voices);
    let replacement = &report.replacement;
    println!("replacement_composition=separate-admitted retained_mixer=true epoch_before={} epoch_after={} paused_render_reports={} progressing_snapshots={} retire_calls={} open_calls={} premature_recovery_refused={}",
        replacement.epoch_before, replacement.epoch_after, replacement.paused_render_reports.len(),
        replacement.snapshots.len(), replacement.retire_calls, replacement.open_calls, replacement.premature_recovery_refused);
    println!(
        "practice_replacement=refused refusal_mapping_calls={} refusal_retire_calls={} reason={:?}",
        report.refusal_mapping_calls, report.refusal_retire_calls, report.replacement_refusal
    );
    println!("software_only=true physical_devices=unavailable native_latency=unavailable gpu=unavailable network=unavailable wall_clock_soak=unavailable");
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
