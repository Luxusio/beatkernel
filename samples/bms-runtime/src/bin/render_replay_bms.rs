//! Recorded BMS sounds to newly created raw f32le; no native playback.
use beatkernel::{
    audio::{AudioFormat, AudioLimits, PcmLimits},
    input::CodecLimits,
    replay::codec::ReplayCodecLimits,
    time::Duration,
};
use beatkernel_bms_runtime::{
    ChannelPolicy, load_prepared_for_replay, offline::OfflineOptions, replay_playback::read_replay,
    replay_render::render_replay,
};
use std::{collections::HashSet, error::Error, fs::File, io::Write, path::PathBuf};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct Options {
    chart: PathBuf,
    replay: PathBuf,
    output: PathBuf,
    format: AudioFormat,
    render: OfflineOptions,
    preroll: Duration,
    max_records: usize,
    max_bytes: usize,
}
fn positive_usize(value: &str) -> Result<usize> {
    let count: usize = value.parse()?;
    if count == 0 {
        return Err("limit must be a positive usize".into());
    }
    Ok(count)
}
fn parse(args: &[String]) -> Result<Options> {
    let (mut chart, mut replay, mut output) = (None, None, None);
    let (mut seconds, mut rate) = (None, None);
    let mut channels = 2u16;
    let mut preroll = 3_000_000_000i64;
    let mut block_frames = 4096usize;
    let mut command_capacity = 65536usize;
    let mut voices = 4096usize;
    let mut max_records = 1_000_000usize;
    let mut max_bytes = 64 * 1024 * 1024usize;
    let mut seen = HashSet::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if !seen.insert(flag.as_str()) {
            return Err(format!("duplicate option {flag}").into());
        }
        let value = args.next().ok_or("each option requires a value")?;
        match flag.as_str() {
            "--chart" | "--replay" | "--output" => {
                if value.is_empty() {
                    return Err(format!("{flag} requires a nonempty path").into());
                }
                let path = Some(PathBuf::from(value));
                match flag.as_str() {
                    "--chart" => chart = path,
                    "--replay" => replay = path,
                    _ => output = path,
                }
            }
            "--seconds" => {
                let count: u64 = value.parse()?;
                if count == 0 {
                    return Err("seconds must be positive u64".into());
                }
                seconds = Some(count);
            }
            "--rate" => rate = Some(value.parse::<u32>()?),
            "--channels" => channels = value.parse()?,
            "--preroll-ns" => {
                preroll = value.parse()?;
                if preroll < 0 {
                    return Err("preroll must be nonnegative i64 nanoseconds".into());
                }
            }
            "--block-frames" => block_frames = positive_usize(value)?,
            "--command-capacity" => command_capacity = positive_usize(value)?,
            "--voices" => voices = positive_usize(value)?,
            "--max-records" => max_records = positive_usize(value)?,
            "--max-bytes" => max_bytes = positive_usize(value)?,
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    let seconds = seconds.ok_or("--seconds N is required")?;
    let format = AudioFormat::new(rate.ok_or("--rate HZ is required")?, channels)?;
    let frames = seconds
        .checked_mul(u64::from(format.sample_rate()))
        .ok_or("render frame extent overflow")?;
    i64::try_from(i128::from(seconds) * 1_000_000_000)
        .map_err(|_| "render timestamp extent overflow")?;
    frames
        .checked_mul(u64::from(channels))
        .and_then(|samples| samples.checked_mul(4))
        .ok_or("render byte extent overflow")?;
    AudioLimits::new(
        command_capacity,
        voices,
        command_capacity,
        block_frames,
        command_capacity,
    )?;
    Ok(Options {
        chart: chart.ok_or("--chart PATH is required")?,
        replay: replay.ok_or("--replay PATH is required")?,
        output: output.ok_or("--output NEW_PATH is required")?,
        format,
        render: OfflineOptions {
            frames,
            block_frames,
            command_capacity,
            max_voices: voices,
        },
        preroll: Duration::from_nanos(preroll),
        max_records,
        max_bytes,
    })
}
fn run(options: Options) -> Result<()> {
    let limits = ReplayCodecLimits::new(
        options.max_bytes,
        options.max_records,
        4096,
        CodecLimits::new(65536, 32768)?,
    )?;
    let file = read_replay(&mut File::open(&options.replay)?, limits)?;
    let prepared = load_prepared_for_replay(
        &options.chart,
        options.format,
        PcmLimits::new(
            64 * 1024 * 1024,
            256 * 1024 * 1024,
            beatkernel_bms_runtime::DEFAULT_BMS_PCM_SAMPLES,
        )?,
        ChannelPolicy::Exact,
        &file,
        limits,
    )?;
    for warning in &prepared.source.warnings {
        eprintln!("BMS warning line {}: {}", warning.line, warning.message);
    }
    let mut output = File::create_new(&options.output)?;
    let report = render_replay(
        prepared,
        file,
        limits,
        options.render,
        options.preroll,
        &mut output,
    )?;
    println!(
        "last successful core RenderReport={:?}; execution distinct from admission/native/physical delivery",
        report.last_render
    );
    output.flush()?;
    println!(
        "logical replay sounds rendered to new raw interleaved f32le {:?}: frames={} format={:?} commands_admitted={} full_replay_results={} full_replay_hits={} recorded_until={:?} final_judge_hash={:#018x}; no native playback or original dropped-audio reproduction",
        options.output,
        report.frames,
        report.format,
        report.commands_admitted,
        report.judge_results,
        report.hits,
        report.recorded_until,
        report.final_judge_hash
    );
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    run_args(&args)
}

pub(crate) fn run_args(args: &[String]) -> Result<()> {
    if args.is_empty() || args == ["--help"] {
        println!(
            "render_replay_bms --chart PATH --replay PATH --output NEW_PATH --seconds N --rate HZ [--channels N --preroll-ns N --block-frames N --command-capacity N --voices N --max-records N --max-bytes N]\nDefaults: channels 2 (Exact asset layout), preroll 3000000000ns, block frames 4096, command capacity 65536, voices 4096, max records 1000000, max bytes 67108864.\nSeconds is a positive finite checked output duration including preroll; rate is nonzero. Preroll is nonnegative i64 nanoseconds; capacities are positive checked integers within core limits.\nLoads bounded WAV assets and matching logical replay, writes new raw interleaved f32le without overwriting. Failures may leave a partial new file.\nJudge counts/hash describe the full replay; admitted sounds and PCM are cut off by the requested extent. No native playback, physical timing, or past dropped-audio reproduction."
        );
        return Ok(());
    }
    run(parse(args)?)
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn args(extra: &[&str]) -> Vec<String> {
        [
            "--chart",
            "chart.bms",
            "--replay",
            "session.bkr",
            "--output",
            "new.f32le",
            "--seconds",
            "2",
            "--rate",
            "4",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(str::to_owned)
        .collect()
    }
    #[test]
    fn defaults_and_explicit_pcm_options() {
        let parsed = parse(&args(&[])).unwrap();
        assert_eq!(parsed.render.frames, 8);
        assert_eq!(parsed.format.channels(), 2);
        assert_eq!(parsed.preroll.as_nanos(), 3_000_000_000);
        assert_eq!(parsed.render.block_frames, 4096);
        assert_eq!(parsed.render.command_capacity, 65536);
        assert_eq!(parsed.render.max_voices, 4096);
        assert_eq!(parsed.max_records, 1_000_000);
        assert_eq!(parsed.max_bytes, 64 * 1024 * 1024);
        let supplied = parse(&args(&[
            "--channels",
            "1",
            "--preroll-ns",
            "0",
            "--block-frames",
            "1",
            "--command-capacity",
            "2",
            "--voices",
            "1",
            "--max-records",
            "3",
            "--max-bytes",
            "4096",
        ]))
        .unwrap();
        assert_eq!(supplied.format.channels(), 1);
        assert_eq!(supplied.preroll, Duration::ZERO);
        assert_eq!(supplied.render.block_frames, 1);
        assert_eq!(supplied.render.command_capacity, 2);
        assert_eq!(supplied.render.max_voices, 1);
        assert_eq!(supplied.max_records, 3);
        assert_eq!(supplied.max_bytes, 4096);
    }
    #[test]
    fn strict_paths_flags_and_positive_bounded_capacities() {
        assert!(parse(&[]).is_err());
        assert!(parse(&args(&["--unknown", "x"])).is_err());
        assert!(parse(&args(&["--voices"])).is_err());
        for flag in ["--chart", "--replay", "--output", "--seconds", "--rate"] {
            assert!(parse(&args(&[flag, "1"])).is_err());
        }
        for flag in [
            "--channels",
            "--preroll-ns",
            "--block-frames",
            "--command-capacity",
            "--voices",
            "--max-records",
            "--max-bytes",
        ] {
            assert!(parse(&args(&[flag, "1", flag, "2"])).is_err());
        }
        for flag in [
            "--block-frames",
            "--command-capacity",
            "--voices",
            "--max-records",
            "--max-bytes",
        ] {
            for value in ["0", "-1", "184467440737095516160"] {
                assert!(parse(&args(&[flag, value])).is_err());
            }
        }
        for (flag, value) in [
            ("--channels", "0"),
            ("--channels", "33"),
            ("--preroll-ns", "-1"),
            ("--preroll-ns", "9223372036854775808"),
            ("--block-frames", "1048577"),
            ("--command-capacity", "65537"),
            ("--voices", "4097"),
        ] {
            assert!(parse(&args(&[flag, value])).is_err());
        }
        for flag in ["--chart", "--replay", "--output"] {
            let mut invalid = args(&[]);
            let index = invalid.iter().position(|value| value == flag).unwrap();
            invalid[index + 1].clear();
            assert!(parse(&invalid).is_err());
        }
    }
    #[test]
    fn finite_render_extent_is_checked_before_opening_output() {
        for (flag, value) in [
            ("--seconds", "0"),
            ("--seconds", "18446744073709551615"),
            ("--rate", "0"),
            ("--rate", "4294967296"),
        ] {
            let mut invalid = args(&[]);
            let index = invalid.iter().position(|option| option == flag).unwrap();
            invalid[index + 1] = value.into();
            assert!(parse(&invalid).is_err());
        }
        assert!(parse(&args(&["--preroll-ns", "9223372036854775807"])).is_ok());
    }
}
