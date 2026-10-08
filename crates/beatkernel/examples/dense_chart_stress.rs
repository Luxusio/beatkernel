//! Bounded software stress through the actual judge, replay and visual projector.
#[path = "dense_chart_stress/workload.rs"]
mod workload;

use std::{error::Error, str::FromStr};
use workload::{Options, Report};

const HELP: &str = "BeatKernel dense chart and repeated seek software stress
Usage: cargo run --release -p beatkernel --example dense_chart_stress -- [options]
  --notes N        chart objects, 1..100000 (default 20000)
  --lanes N        chart lanes, 1..64 (default 8)
  --seek-cycles N  seek and projection checks, 1..1000 (default 16)
  --origin-ns N    absolute song origin, 0..604800000000000 (default 0)
  --help          show this help; use alone
Each flag may appear once and requires one integer value.
notes * (seek_cycles + 1) must not exceed 5000000.
Software only: audio, native clocks/latency, GPU rendering, physical devices,
whole-player behavior and actual long-duration wall-clock soak are unavailable.
Long origins test timestamp placement only; lanes are not multiplayer members.
Phase nanoseconds are informational observations with no performance threshold.";

fn integer<T: FromStr>(flag: &str, value: &str) -> Result<T, Box<dyn Error>> {
    value
        .parse()
        .map_err(|_| format!("{flag} requires a representable integer; use --help").into())
}

fn parse(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut options = Options::default();
    let mut seen = [false; 4];
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--notes" => 0,
            "--lanes" => 1,
            "--seek-cycles" => 2,
            "--origin-ns" => 3,
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
        match slot {
            0 => options.notes = integer(flag, value)?,
            1 => options.lanes = integer(flag, value)?,
            2 => options.seek_cycles = integer(flag, value)?,
            3 => options.origin_ns = integer(flag, value)?,
            _ => unreachable!(),
        }
        index += 2;
    }
    options.validate()?;
    Ok(options)
}

fn summary(report: &Report) {
    println!(
        "benchmark_schema=1 workload_id=beatkernel-dense-chart-v1 debug_assertions={}",
        cfg!(debug_assertions)
    );
    println!(
        "notes={} lanes={} seek_cycles={} origin_ns={}",
        report.options.notes,
        report.options.lanes,
        report.options.seek_cycles,
        report.options.origin_ns
    );
    println!(
        "hold_count={} record_count={} result_count={} checkpoint_count={}",
        report.hold_count, report.record_count, report.result_count, report.checkpoint_count
    );
    println!(
        "seek_checks={} projection_checks={} max_visible={} final_engine_hash={} retained_probes={}",
        report.seek_checks,
        report.projection_checks,
        report.max_visible,
        report.final_engine_hash,
        report.probes.len()
    );
    if let Some(probe) = report.probes.last() {
        println!(
            "last_probe_target_ns={} last_probe_cursor={} last_probe_result_count={} last_probe_engine_hash={} last_probe_visible_count={}",
            probe.target_ns,
            probe.cursor,
            probe.result_count,
            probe.engine_hash,
            probe.objects.len()
        );
    }
    println!(
        "setup_ns={} recording_ns={} seek_ns={} projection_ns={} verification_ns={}",
        report.setup_ns,
        report.record_ns,
        report.seek_ns,
        report.projection_ns,
        report.verification_ns
    );
    println!("software-only; phase timings are informational observations with no performance threshold, ranking or universal zero-cost claim");
    println!("audio=unavailable native_clocks_latency=unavailable gpu_rendering=unavailable physical_devices=unavailable whole_player_behavior=unavailable actual_wall_clock_soak=unavailable");
    println!(
        "long origins test absolute timestamp placement only; lanes are not multiplayer members"
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!("{HELP}");
        return Ok(());
    }
    let report = workload::run(parse(&args)?)?;
    summary(&report);
    Ok(())
}
