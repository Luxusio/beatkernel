//! Primary command routing within the existing application crate.
use crate::Result;
use beatkernel::time::Timestamp;
use beatkernel_bms_runtime::{
    competition::Competition,
    competition_live::{CompetitionOptions, load_chart, replay_limits},
    replay_playback::{read_replay, reconstruct},
};
use std::{fs::File, path::Path};

pub(super) fn run(args: &[String]) -> Result<()> {
    let Some(command) = args.first() else {
        return desktop(&["--library".into(), ".".into()]);
    };
    let rest = &args[1..];
    match command.as_str() {
        "--help" | "help" => {
            help();
            Ok(())
        }
        "play" => play(rest),
        "player" => desktop(rest),
        "replay" => crate::replay_tool::run_args(rest),
        "play-replay" => crate::replay_player::run_args(rest),
        "render-replay" => crate::replay_renderer::run_args(rest),
        "render" => crate::render_offline_args(rest),
        "compete" => compare(rest),
        // Keep the old positional renderer available, including charts with
        // arbitrary names; explicit modes use their own strict option parsers.
        _ if args.len() >= 4 && !command.starts_with('-') => crate::render_offline_args(args),
        _ => Err(format!("unknown application mode {command}; use --help").into()),
    }
}

fn help() {
    println!(
        "BeatKernel BMS application\n\
play [native options] [--ghost-self REPLAY] [--ghost-other REPLAY] [--mp-host IP:PORT | --mp-join IP:PORT] [--mp-timeout-ms N]\n\
player [--library DIR | --chart PATH] [native options] [--ui-lookahead-ms N] [--ui-fps N]  Graphical player\n\
replay [--chart PATH --replay PATH ...]                  Inspect recorded play\n\
play-replay [native replay output options]             Play recorded sounds\n\
render CHART NEW_OUTPUT SECONDS RATE [CHANNELS]         Offline synthetic render\n\
render-replay [recorded PCM output options]            Render recorded sounds\n\
compete --chart PATH --local-replay PATH [--ghost-self PATH] [--ghost-other PATH] [--song-ns N]\n\
Use MODE --help for mode options. Primary play/player resolve omitted devices automatically; standalone native tools keep exact option requirements.\n\
Saved opponents require the same compiled chart and judging profile. Multiplayer is two-peer casual progress exchange; song starts are local and scores are self-reported."
    );
}

fn desktop(args: &[String]) -> Result<()> {
    #[cfg(feature = "desktop")]
    {
        crate::desktop::run(
            args,
            native,
            validate_native,
            crate::devices_native::query,
            native_replay,
            validate_replay,
        )
    }
    #[cfg(not(feature = "desktop"))]
    {
        let _ = args;
        Err("graphical player requires the desktop Cargo feature; use --features desktop or select --help for headless modes".into())
    }
}

fn play(args: &[String]) -> Result<()> {
    if args.is_empty() || args == ["--help"] {
        println!(
            "Competition options: --ghost-self PATH and --ghost-other PATH (up to 8 total); --mp-host IP:PORT or --mp-join IP:PORT, optional --mp-timeout-ms 100..120000 (default10000). Explicit numeric addresses; host port must be nonzero. Peer loss disables multiplayer while local play continues."
        );
        return native(args);
    }
    // Window/input/run-loop objects are created on this game owner. The main
    // thread remains available for a future graphical UI, and audio retains
    // its native worker/callback ownership. No native handles cross threads.
    let args = args.to_vec();
    let worker = std::thread::Builder::new()
        .name("bms-game".into())
        .spawn(move || native(&args).map_err(|error| error.to_string()))?;
    println!("BMS game thread started; native output uses its audio worker/callback.");
    worker
        .join()
        .map_err(|_| "BMS game thread panicked")?
        .map_err(Into::into)
}

fn native(args: &[String]) -> Result<()> {
    let prepared;
    let args = if args.is_empty() || args == ["--help"] {
        args
    } else {
        prepared = crate::auto_native::prepare(args)?;
        &prepared
    };
    #[cfg(target_os = "windows")]
    return crate::windows_play::run_args(args);
    #[cfg(target_os = "linux")]
    return crate::linux_play::run_args(args);
    #[cfg(target_os = "macos")]
    return crate::macos_play::run_args(args);
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = args;
        Err("native BMS play requires Windows, Linux or macOS".into())
    }
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub(super) fn validate_native(args: &[String]) -> Result<()> {
    let prepared = crate::auto_native::syntax_args(args)?;
    let args = &prepared;
    #[cfg(target_os = "windows")]
    return crate::windows_play::validate_args(args);
    #[cfg(target_os = "linux")]
    return crate::linux_play::validate_args(args);
    #[cfg(target_os = "macos")]
    return crate::macos_play::validate_args(args);
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = args;
        Err("native BMS play requires Windows, Linux or macOS".into())
    }
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
fn native_replay(args: &[String]) -> Result<()> {
    let projected = crate::auto_native::prepare_replay(args)?;
    crate::replay_player::run_args(&projected)
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub(super) fn validate_replay(args: &[String]) -> Result<()> {
    let projected = crate::auto_native::syntax_replay(args)?;
    crate::replay_player::validate_args(&projected)
}

fn compare(args: &[String]) -> Result<()> {
    if args.is_empty() || args == ["--help"] {
        println!(
            "compete --chart PATH --local-replay PATH [--ghost-self PATH] [--ghost-other PATH] [--song-ns N]\nCompare actual recorded result prefixes at the selected song time; defaults to local recording's last operation. No missing tail is fabricated, and grade IDs have no implicit weights. Native competition uses play with the same ghost options."
        );
        return Ok(());
    }
    let (options, rest) = CompetitionOptions::extract(args)?;
    if options.network.is_some() {
        return Err("compete inspects saved records; use play for live multiplayer".into());
    }
    if rest.len() % 2 != 0 {
        return Err("competition options require flag/value pairs".into());
    }
    let (mut chart, mut replay, mut song) = (None, None, None);
    for pair in rest.chunks_exact(2) {
        match pair[0].as_str() {
            "--chart" if chart.is_none() && !pair[1].is_empty() => chart = Some(&pair[1]),
            "--local-replay" if replay.is_none() && !pair[1].is_empty() => replay = Some(&pair[1]),
            "--song-ns" if song.is_none() => song = Some(pair[1].parse::<i64>()?),
            flag => return Err(format!("unknown, duplicate or empty option {flag}").into()),
        }
    }
    let source = load_chart(Path::new(chart.ok_or("--chart is required")?))?;
    let limits = replay_limits()?;
    let file = read_replay(
        &mut File::open(replay.ok_or("--local-replay is required")?)?,
        limits,
    )?;
    let mut competition = Competition::new(file.header.clone(), 8)?;
    options.load_opponents(&source, &mut competition, limits)?;
    let mut local = reconstruct(&source, file, limits)?;
    let target = Timestamp::from_nanos(song.unwrap_or_else(|| {
        local
            .records()
            .last()
            .map_or(0, |record| record.song_time.as_nanos())
    }));
    let cursor = local
        .records()
        .partition_point(|record| record.song_time <= target);
    local.seek_cursor(cursor)?;
    competition.observe(local.results(), target)?;
    println!(
        "local at {}ns: {:?}",
        target.as_nanos(),
        competition.score()
    );
    for opponent in competition.opponents() {
        println!(
            "{:?} ghost={} score={:?} recorded_until={:?}",
            opponent.kind(),
            opponent.label(),
            opponent.score(),
            opponent.recorded_until()
        );
    }
    Ok(())
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn unknown_modes_and_missing_comparison_arguments_are_rejected() {
        assert!(run(&["unknown".into()]).is_err());
        assert!(compare(&["--chart".into(), "x".into()]).is_err());
        assert!(compare(&["--song-ns".into(), "overflow".into()]).is_err());
        assert!(compare(&["--mp-host".into(), "127.0.0.1:9000".into()]).is_err());
    }
}
