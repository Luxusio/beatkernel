//! Bounded logical BMS replay reconstruction; no assets, devices or output files.
use beatkernel::{input::CodecLimits, replay::codec::ReplayCodecLimits, time::Timestamp};
use beatkernel_bms::{parse as parse_chart, ParseOptions};
use beatkernel_bms_runtime::replay_playback::{read_replay, reconstruct};
use std::{
    collections::HashSet,
    error::Error,
    fs::File,
    io::{self, Read},
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
struct Options {
    chart: PathBuf,
    replay: PathBuf,
    max_records: usize,
    max_bytes: usize,
    cursor: Option<usize>,
    song_ns: Option<i64>,
}

fn parse(args: &[String]) -> Result<Options> {
    let mut chart = None;
    let mut replay = None;
    let mut max_records = 1_000_000usize;
    let mut max_bytes = 64 * 1024 * 1024usize;
    let mut cursor = None;
    let mut song_ns = None;
    let mut seen = HashSet::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if !seen.insert(flag.as_str()) {
            return Err(format!("duplicate option {flag}").into());
        }
        let value = args.next().ok_or("each option requires a value")?;
        match flag.as_str() {
            "--chart" | "--replay" => {
                if value.is_empty() {
                    return Err(format!("{flag} requires a nonempty path").into());
                }
                if flag == "--chart" {
                    chart = Some(PathBuf::from(value));
                } else {
                    replay = Some(PathBuf::from(value));
                }
            }
            "--max-records" => {
                max_records = value.parse()?;
                if max_records == 0 {
                    return Err("max records must be a positive usize".into());
                }
            }
            "--max-bytes" => {
                max_bytes = value.parse()?;
                if max_bytes == 0 {
                    return Err("max bytes must be a positive usize".into());
                }
            }
            "--cursor" => cursor = Some(value.parse()?),
            "--song-ns" => song_ns = Some(value.parse()?),
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    if cursor.is_some() && song_ns.is_some() {
        return Err("--cursor and --song-ns are mutually exclusive".into());
    }
    Ok(Options {
        chart: chart.ok_or("--chart PATH is required")?,
        replay: replay.ok_or("--replay PATH is required")?,
        max_records,
        max_bytes,
        cursor,
        song_ns,
    })
}

// Enforce the byte bound while reading, including growing files; metadata is not
// used as evidence of extent. The scratch buffer is fixed and UTF-8 is explicit.
fn read_chart_text(reader: &mut impl Read, max_bytes: usize) -> Result<String> {
    let mut bytes = Vec::new();
    let mut scratch = [0u8; 8192];
    loop {
        let count = match reader.read(&mut scratch) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        let extent = bytes
            .len()
            .checked_add(count)
            .ok_or("chart byte extent overflow")?;
        if extent > max_bytes {
            return Err(format!("BMS text exceeds {max_bytes} bytes").into());
        }
        if extent > bytes.capacity() {
            let capacity = extent
                .max(bytes.capacity().saturating_mul(2))
                .min(max_bytes);
            bytes.try_reserve_exact(capacity - bytes.len())?;
        }
        bytes.extend_from_slice(&scratch[..count]);
    }
    Ok(String::from_utf8(bytes)?)
}

fn run(options: Options) -> Result<()> {
    let limits = ReplayCodecLimits::new(
        options.max_bytes,
        options.max_records,
        4096,
        CodecLimits::new(65536, 32768)?,
    )?;
    let chart_options = ParseOptions::default();
    let text = read_chart_text(&mut File::open(&options.chart)?, chart_options.max_bytes)?;
    let chart = parse_chart(&text, chart_options)?;
    for warning in &chart.warnings {
        eprintln!("BMS warning line {}: {}", warning.line, warning.message);
    }
    let file = read_replay(&mut File::open(&options.replay)?, limits)?;
    let mut session = reconstruct(&chart, file, limits)?;
    if let Some(cursor) = options.cursor {
        session.seek_cursor(cursor)?;
    } else if let Some(nanos) = options.song_ns {
        session.seek(Timestamp::from_nanos(nanos))?;
    }
    println!("logical BMS replay reconstruction only; no asset loading, native audio or physical timing claim");
    println!(
        "cursor={} records={} judge_results={} engine_hash={:#018x} effective_time={:?}",
        session.cursor(),
        session.records().len(),
        session.results().len(),
        session.engine().stable_hash()?,
        session.engine().effective_song_time(),
    );
    for event in session.results() {
        println!("judge={event:?}");
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!("replay_bms --chart PATH --replay PATH [--max-records N] [--max-bytes N] [--cursor N | --song-ns N]\nLogical replay inspection through the same BMS JudgeEngine; no PCM assets, native devices or output writes.\nDefaults: max records 1000000, max bytes 67108864 (64 MiB); limits require positive usize values. Chart uses bounded default BMS parser limits (8 MiB text).\nCursor is an exact operation boundary including zero; song-ns is signed nanoseconds and may produce the core's explicit boundary timeout advance.\nRequires matching chart, stored profile, rules and runtime version; setup hash is noncryptographic. Physical input-to-sound timing remains unknown.");
        return Ok(());
    }
    run(parse(&args)?)
}

#[cfg(test)]
mod fixtures {
    use super::*;

    fn args(extra: &[&str]) -> Vec<String> {
        ["--chart", "chart.bms", "--replay", "session.bkr"]
            .into_iter()
            .chain(extra.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn defaults_and_exact_cursor_or_signed_time() {
        let defaults = parse(&args(&[])).unwrap();
        assert_eq!(defaults.chart, PathBuf::from("chart.bms"));
        assert_eq!(defaults.replay, PathBuf::from("session.bkr"));
        assert_eq!(defaults.max_records, 1_000_000);
        assert_eq!(defaults.max_bytes, 64 * 1024 * 1024);
        assert!(defaults.cursor.is_none() && defaults.song_ns.is_none());
        assert_eq!(parse(&args(&["--cursor", "0"])).unwrap().cursor, Some(0));
        for value in ["-9223372036854775808", "-1", "0", "9223372036854775807"] {
            assert_eq!(
                parse(&args(&["--song-ns", value])).unwrap().song_ns,
                Some(value.parse().unwrap())
            );
        }
        assert!(parse(&args(&["--cursor", "0", "--song-ns", "0"])).is_err());
    }

    #[test]
    fn strict_paths_flags_and_representable_positive_caps() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--chart".into(), "x".into()]).is_err());
        assert!(parse(&["--chart".into(), "".into(), "--replay".into(), "x".into()]).is_err());
        assert!(parse(&["--chart".into(), "x".into(), "--replay".into(), "".into()]).is_err());
        assert!(parse(&args(&["--unknown", "x"])).is_err());
        assert!(parse(&args(&["--cursor"])).is_err());
        assert!(parse(&args(&["--cursor", "-1"])).is_err());
        assert!(parse(&args(&["--song-ns", "9223372036854775808"])).is_err());
        for flag in [
            "--chart",
            "--replay",
            "--max-records",
            "--max-bytes",
            "--cursor",
            "--song-ns",
        ] {
            let value = if flag == "--chart" || flag == "--replay" {
                "x"
            } else {
                "1"
            };
            let extra = if flag == "--chart" || flag == "--replay" {
                vec![flag, value]
            } else {
                vec![flag, value, flag, value]
            };
            assert!(parse(&args(&extra)).is_err());
        }
        for flag in ["--max-records", "--max-bytes"] {
            assert!(parse(&args(&[flag, "1"])).is_ok());
            for value in ["0", "-1", "184467440737095516160"] {
                assert!(parse(&args(&[flag, value])).is_err());
            }
        }
    }

    #[test]
    fn chart_reader_checks_stream_extent_utf8_and_errors() {
        let text = b"#BPM 120\n";
        assert_eq!(
            read_chart_text(&mut &text[..], text.len()).unwrap(),
            "#BPM 120\n"
        );
        assert!(read_chart_text(&mut &text[..], text.len() - 1).is_err());
        assert!(read_chart_text(&mut &[0xff][..], 1).is_err());
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("read fixture failure"))
            }
        }
        assert!(read_chart_text(&mut Broken, 16).is_err());
    }
}
