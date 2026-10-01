//! Unified BMS application; legacy positional arguments retain offline rendering.
mod app;
#[cfg(feature = "desktop")]
mod desktop;
#[allow(dead_code)]
#[path = "bin/linux_bms.rs"]
mod linux_play;
#[allow(dead_code)]
#[path = "bin/macos_bms.rs"]
mod macos_play;
#[allow(dead_code)]
#[path = "bin/play_replay_bms.rs"]
mod replay_player;
#[allow(dead_code)]
#[path = "bin/render_replay_bms.rs"]
mod replay_renderer;
#[allow(dead_code)]
#[path = "bin/replay_bms.rs"]
mod replay_tool;
#[allow(dead_code)]
#[path = "bin/windows_bms.rs"]
mod windows_play;
use beatkernel::audio::{AudioFormat, PcmLimits};
use beatkernel_bms_runtime::{
    load_prepared,
    offline::{render_offline, OfflineOptions},
    ChannelPolicy,
};
use std::{fs::File, io::Write, path::Path};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    app::run(&args)
}

fn render_offline_args(args: &[String]) -> Result<()> {
    if args == ["--help"] {
        println!("beatkernel-bms-runtime CHART.bms NEW_OUTPUT.f32le SECONDS RATE [CHANNELS]\nOffline synthetic input with shared bounded WAV preparation and chronological PCM rendering; no native playback.\nCHANNELS defaults to 2; choose 1 for mono assets. Asset channels must match exactly.\nOutput is newly created raw interleaved f32le; rendering failure may leave a partial file.\nConcurrent voices and outstanding commands are bounded independently from total chart notes.");
        return Ok(());
    }
    if !(4..=5).contains(&args.len()) {
        return Err(
            "usage: beatkernel-bms-runtime CHART NEW_OUTPUT SECONDS RATE [CHANNELS]".into(),
        );
    }
    let seconds: u64 = args[2].parse()?;
    let rate: u32 = args[3].parse()?;
    let channels: u16 = args
        .get(4)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(2);
    if seconds == 0 {
        return Err("duration must be positive".into());
    }
    let format = AudioFormat::new(rate, channels)?;
    let frames = seconds
        .checked_mul(u64::from(rate))
        .ok_or("render extent overflow")?;
    let prepared = load_prepared(
        Path::new(&args[0]),
        format,
        PcmLimits::new(64 * 1024 * 1024, 256 * 1024 * 1024, 1295)?,
        ChannelPolicy::Exact,
    )?;
    for warning in &prepared.source.warnings {
        eprintln!("BMS warning line {}: {}", warning.line, warning.message);
    }
    let options = OfflineOptions {
        frames,
        block_frames: 4096,
        command_capacity: 65_536,
        max_voices: 4096,
    };
    let mut output = File::create_new(&args[1])?;
    let report = render_offline(prepared, options, &mut output)?;
    output.flush()?;
    println!(
        "synthetic input: {} hits, {} judge results; {} frames, {} channels at {}Hz; raw f32le output; no native playback",
        report.hits, report.judge_results, report.frames, report.format.channels(), report.format.sample_rate()
    );
    println!("last successful core render={:?}", report.last_render);
    Ok(())
}
