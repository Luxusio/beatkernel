//! Immutable native invocations for fresh graphical sessions, without device ownership.
use crate::{
    practice::PracticeStart,
    practice_loop::PracticeLoop,
    settings::{MAX_FIELDS, MAX_TOTAL_BYTES, MAX_VALUE_BYTES},
};
use std::{path::Path, sync::Arc};

/// A session and its checked retry ordinal. Native parsers admit actual options.
#[derive(Clone, Debug)]
pub struct SessionLaunch {
    original: Arc<[String]>,
    args: Vec<String>,
    attempt: u32,
}
impl SessionLaunch {
    /// Pins a chart invocation before creating game or audio owners.
    pub fn new(args: Vec<String>) -> Result<Self, String> {
        validate(&args)?;
        Ok(Self {
            original: args.clone().into(),
            args,
            attempt: 0,
        })
    }
    pub fn args(&self) -> &[String] {
        &self.args
    }
    /// Zero is the original session; positive numbers identify retries.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
    /// Derives every invocation from the original, never a previous suffix.
    /// No file existence check or filename fallback occurs here.
    pub fn retry(&self) -> Result<Self, String> {
        let attempt = self
            .attempt
            .checked_add(1)
            .ok_or("retry ordinal exhausted")?;
        let mut args = self.original.to_vec();
        for pair in args.chunks_exact_mut(2) {
            if pair[0] == "--record-replay" {
                if pair[1].ends_with('/') || (cfg!(windows) && pair[1].ends_with('\\')) {
                    return Err("recording base must name a file for retry".into());
                }
                let base = Path::new(&pair[1]);
                let mut stem = base
                    .file_stem()
                    .filter(|stem| !stem.is_empty())
                    .ok_or("recording base requires a filename for retry")?
                    .to_os_string();
                stem.push(format!(".retry{attempt}.bkr"));
                pair[1] = base
                    .with_file_name(stem)
                    .to_str()
                    .ok_or("retry recording path must be UTF-8")?
                    .to_owned();
            }
        }
        validate(&args)?;
        Ok(Self {
            original: self.original.clone(),
            args,
            attempt,
        })
    }
    /// Creates one fresh finite owner from the pinned original invocation.
    /// Repeating a region never accumulates prior endpoints or capture suffixes.
    pub fn retry_loop(&self, region: PracticeLoop) -> Result<Self, String> {
        let mut retry = self.retry()?;
        let mut indices = [None, None];
        for (index, pair) in retry.args.chunks_exact(2).enumerate() {
            match pair[0].as_str() {
                "--replay" | "--mp-host" | "--mp-join" => {
                    return Err("finite practice loops require live nonnetwork playback".into());
                }
                "--start-ns" | "--end-ns" => {
                    let slot = usize::from(pair[0] == "--end-ns");
                    if indices[slot].replace(index * 2 + 1).is_some() {
                        return Err(
                            "practice loop requires at most one start and end option".into()
                        );
                    }
                }
                _ => {}
            }
        }
        for (slot, flag, value) in [
            (0, "--start-ns", region.start().nanoseconds()),
            (1, "--end-ns", region.end().nanoseconds()),
        ] {
            if let Some(index) = indices[slot] {
                retry.args[index] = value.to_string();
            } else {
                retry.args.extend([flag.into(), value.to_string()]);
            }
        }
        validate(&retry.args)?;
        Ok(retry)
    }
    /// Creates a fresh retry at an exact bookmark while retaining the original
    /// invocation for subsequent ordinary retries and recording path derivation.
    pub fn retry_from(&self, start: PracticeStart) -> Result<Self, String> {
        let mut retry = self.retry()?;
        let mut start_index = None;
        for (index, pair) in retry.args.chunks_exact(2).enumerate() {
            if pair[0] == "--replay" {
                return Err("replay watching cannot restart from a practice bookmark".into());
            }
            if pair[0] == "--start-ns" && start_index.replace(index * 2 + 1).is_some() {
                return Err("practice restart requires at most one start option".into());
            }
        }
        let value = start.nanoseconds().to_string();
        if let Some(index) = start_index {
            retry.args[index] = value;
        } else {
            retry.args.push("--start-ns".into());
            retry.args.push(value);
        }
        validate(&retry.args)?;
        Ok(retry)
    }
}
fn validate(args: &[String]) -> Result<(), String> {
    if args.len() % 2 != 0 || args.len() / 2 > MAX_FIELDS + 1 {
        return Err("session invocation exceeds flag/value pair limit".into());
    }
    let mut charts = 0;
    let mut captures = 0;
    let mut total = 0usize;
    for pair in args.chunks_exact(2) {
        if !pair[0].starts_with("--")
            || pair[0].len() > 128
            || pair[0].chars().any(|c| c.is_control() || c.is_whitespace())
            || pair[1].is_empty()
            || pair[1].len() > MAX_VALUE_BYTES
            || pair[1]
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        {
            return Err("invalid bounded session flag/value".into());
        }
        total = total
            .checked_add(pair[1].len())
            .ok_or("session byte count overflow")?;
        match pair[0].as_str() {
            "--chart" => charts += 1,
            "--record-replay" => captures += 1,
            _ => {}
        }
    }
    if charts != 1 || captures > 1 || total > MAX_TOTAL_BYTES + MAX_VALUE_BYTES {
        return Err("session requires one chart and at most one bounded recording path".into());
    }
    Ok(())
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).into()).collect()
    }
    fn region(start: i64, end: i64) -> PracticeLoop {
        PracticeLoop::new(
            PracticeStart::from_nanoseconds(start).unwrap(),
            PracticeStart::from_nanoseconds(end).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn finite_retries_replace_both_endpoints_and_derive_capture_from_pinned_original() {
        let original = args(&[
            "--chart",
            "songs/a.bms",
            "--start-ns",
            "123",
            "--end-ns",
            "456",
            "--local-player",
            "4294967295:keyboard:path",
            "--local-player",
            "7:other",
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--backend",
            "wasapi",
            "--buffer-frames",
            "512",
            "--preroll-ns",
            "3000000000",
            "--record-replay",
            "records/my.take.bkr",
        ]);
        let launch = SessionLaunch::new(original.clone()).unwrap();
        let first = launch
            .retry_loop(region(72_000_000_000_001, 604_800_000_000_001))
            .unwrap();
        let mut expected = original.clone();
        expected[3] = "72000000000001".into();
        expected[5] = "604800000000001".into();
        *expected.last_mut().unwrap() = "records/my.take.retry1.bkr".into();
        assert_eq!(first.args(), expected);
        let second = first.retry_loop(region(i64::MAX - 1, i64::MAX)).unwrap();
        expected[3] = (i64::MAX - 1).to_string();
        expected[5] = i64::MAX.to_string();
        *expected.last_mut().unwrap() = "records/my.take.retry2.bkr".into();
        assert_eq!(second.args(), expected);
        assert_eq!(second.attempt(), 2);
        assert!(Arc::ptr_eq(&launch.original, &second.original));
        let ordinary = second.retry().unwrap();
        let mut pinned = original.clone();
        *pinned.last_mut().unwrap() = "records/my.take.retry3.bkr".into();
        assert_eq!(ordinary.args(), pinned);
        assert_eq!(launch.args(), original);
        assert_eq!(launch.attempt(), 0);
        assert_eq!(first.args()[3], "72000000000001");
    }
    #[test]
    fn finite_retry_appends_missing_endpoints_once_and_ordinary_retry_restores_unlimited() {
        for original in [
            args(&["--chart", "a.bms"]),
            args(&["--chart", "a.bms", "--start-ns", "7"]),
            args(&["--chart", "a.bms", "--end-ns", "9"]),
        ] {
            let launch = SessionLaunch::new(original.clone()).unwrap();
            let first = launch.retry_loop(region(0, 1)).unwrap();
            let second = first.retry_loop(region(2, 3)).unwrap();
            assert_eq!(first.args().len(), second.args().len());
            for (flag, value) in [("--start-ns", "2"), ("--end-ns", "3")] {
                let pairs: Vec<_> = second
                    .args()
                    .chunks_exact(2)
                    .filter(|pair| pair[0] == flag)
                    .collect();
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0][1], value);
            }
            assert_eq!(second.retry().unwrap().args(), original);
            assert_eq!(launch.args(), original);
        }
    }
    #[test]
    fn finite_retry_duplicate_unsupported_modes_and_capacity_fail_without_mutation() {
        for original in [
            args(&["--chart", "a", "--start-ns", "0", "--start-ns", "1"]),
            args(&["--chart", "a", "--end-ns", "1", "--end-ns", "2"]),
            args(&["--chart", "a", "--replay", "watch.bkr"]),
            args(&["--chart", "a", "--mp-host", "127.0.0.1:34567"]),
            args(&["--chart", "a", "--mp-join", "127.0.0.1:34567"]),
        ] {
            let launch = SessionLaunch::new(original.clone()).unwrap();
            assert!(launch.retry_loop(region(0, 1)).is_err());
            assert_eq!(launch.args(), original);
            assert_eq!(launch.attempt(), 0);
        }
        let mut fields = args(&["--chart", "a"]);
        for _ in 0..MAX_FIELDS {
            fields.extend(args(&["--bind", "11:04"]));
        }
        let full = SessionLaunch::new(fields.clone()).unwrap();
        assert!(full.retry_loop(region(0, 1)).is_err());
        assert_eq!(full.args(), fields);
        let mut total = args(&["--chart", "a"]);
        let mut remaining = MAX_TOTAL_BYTES + MAX_VALUE_BYTES - 1;
        while remaining > 0 {
            let bytes = remaining.min(MAX_VALUE_BYTES);
            total.extend(["--bind".into(), "x".repeat(bytes)]);
            remaining -= bytes;
        }
        let full = SessionLaunch::new(total.clone()).unwrap();
        assert!(full.retry_loop(region(i64::MAX - 1, i64::MAX)).is_err());
        assert_eq!(full.args(), total);
        let mut exhausted = SessionLaunch::new(args(&["--chart", "a"])).unwrap();
        exhausted.attempt = u32::MAX;
        assert!(exhausted.retry_loop(region(0, 1)).is_err());
        assert_eq!(exhausted.attempt(), u32::MAX);
        let original = args(&[
            "--chart",
            "a",
            "--record-replay",
            &format!("{}.bkr", "a".repeat(MAX_VALUE_BYTES - 4)),
        ]);
        let full = SessionLaunch::new(original.clone()).unwrap();
        assert!(full.retry_loop(region(0, 1)).is_err());
        assert_eq!(full.args(), original);
    }
    #[test]
    fn finite_asio_retry_preserves_pinned_driver_clock_and_local_ids_for_native_preflight() {
        let original = args(&[
            "--chart",
            "song.bms",
            "--backend",
            "asio",
            "--buffer",
            "frames:64",
            "--device",
            "driver",
            "--asio-system-clock",
            "multimedia",
            "--asio-timer-error-ns",
            "100",
            "--asio-drift-error-ns",
            "200",
            "--asio-latency-error-ns",
            "300",
            "--output-channels",
            "3,1",
            "--local-player",
            "7:path-a",
            "--local-player",
            "4294967295:path-b",
            "--record-replay",
            "take.bkr",
        ]);
        let pinned = SessionLaunch::new(original.clone()).unwrap();
        let first = pinned.retry_loop(region(100, 200)).unwrap();
        for pair in original
            .chunks_exact(2)
            .filter(|p| p[0] != "--record-replay")
        {
            assert!(first.args().chunks_exact(2).any(|actual| actual == pair));
        }
        for pair in [
            ["--start-ns", "100"],
            ["--end-ns", "200"],
            ["--record-replay", "take.retry1.bkr"],
        ] {
            assert!(first.args().chunks_exact(2).any(|actual| actual == pair));
        }
        let second = first.retry_loop(region(300, 400)).unwrap();
        assert_eq!(second.attempt(), 2);
        assert!(
            second
                .args()
                .chunks_exact(2)
                .any(|p| p == ["--record-replay", "take.retry2.bkr"])
        );
        assert_eq!(
            second.retry().unwrap().args()[..original.len() - 2],
            original[..original.len() - 2]
        );
        assert_eq!(pinned.args(), original);
    }
    #[test]
    fn bookmark_retries_preserve_options_capture_base_and_pinned_f5_start() {
        let original = args(&[
            "--chart",
            "songs/a.bms",
            "--start-ns",
            "123",
            "--local-player",
            "4294967295:keyboard:path",
            "--local-player",
            "7:other",
            "--bind",
            "11:04",
            "--bind",
            "12:05",
            "--backend",
            "asio",
            "--buffer-frames",
            "512",
            "--network-player",
            "peer",
            "--record-replay",
            "records/my.take.bkr",
        ]);
        let first = SessionLaunch::new(original.clone()).unwrap();
        let second = first
            .retry_from(PracticeStart::from_nanoseconds(i64::MAX).unwrap())
            .unwrap();
        assert_eq!(second.attempt(), 1);
        let mut expected = original.clone();
        expected[3] = i64::MAX.to_string();
        *expected.last_mut().unwrap() = "records/my.take.retry1.bkr".into();
        assert_eq!(second.args(), expected);
        assert!(Arc::ptr_eq(&first.original, &second.original));
        let third = second
            .retry_from(PracticeStart::from_nanoseconds(604_800_000_000_001).unwrap())
            .unwrap();
        assert_eq!(third.args()[3], "604800000000001");
        assert_eq!(third.args().last().unwrap(), "records/my.take.retry2.bkr");
        let f5 = third.retry().unwrap();
        assert_eq!(f5.args()[3], "123");
        assert_eq!(f5.args().last().unwrap(), "records/my.take.retry3.bkr");
        assert_eq!(first.args(), original);
        assert_eq!(second.args()[3], i64::MAX.to_string());
    }
    #[test]
    fn missing_start_is_appended_once_and_ordinary_retry_restores_absence() {
        let original = args(&["--chart", "a.bms", "--ghost-other", "past.bkr"]);
        let launch = SessionLaunch::new(original.clone()).unwrap();
        let start = PracticeStart::from_nanoseconds(72_000_000_000_001).unwrap();
        let restarted = launch.retry_from(start).unwrap();
        let mut expected = original.clone();
        expected.extend(args(&["--start-ns", "72000000000001"]));
        assert_eq!(restarted.args(), expected);
        let again = restarted
            .retry_from(PracticeStart::from_nanoseconds(0).unwrap())
            .unwrap();
        assert_eq!(again.args().len(), original.len() + 2);
        assert_eq!(again.args().last().unwrap(), "0");
        assert_eq!(again.retry().unwrap().args(), original);
        assert_eq!(launch.args(), original);
    }
    #[test]
    fn watch_duplicate_capacity_and_ordinal_fail_without_changing_pinned_launch() {
        let start = PracticeStart::from_nanoseconds(i64::MAX).unwrap();
        for values in [
            args(&["--chart", "a", "--replay", "old.bkr"]),
            args(&["--chart", "a", "--start-ns", "0", "--start-ns", "1"]),
        ] {
            let launch = SessionLaunch::new(values.clone()).unwrap();
            assert!(launch.retry_from(start).is_err());
            assert_eq!(launch.args(), values);
            assert_eq!(launch.attempt(), 0);
        }
        let mut exhausted = SessionLaunch::new(args(&["--chart", "a"])).unwrap();
        exhausted.attempt = u32::MAX;
        assert!(exhausted.retry_from(start).is_err());
        assert_eq!(exhausted.attempt(), u32::MAX);
        let mut fields = args(&["--chart", "a"]);
        for _ in 0..MAX_FIELDS {
            fields.extend(args(&["--bind", "11:04"]));
        }
        let launch = SessionLaunch::new(fields.clone()).unwrap();
        assert!(launch.retry_from(start).is_err());
        assert_eq!(launch.args(), fields);
        let mut full = args(&["--chart", "a"]);
        let mut remaining = MAX_TOTAL_BYTES + MAX_VALUE_BYTES - 1;
        while remaining > 0 {
            let bytes = remaining.min(MAX_VALUE_BYTES);
            full.extend(["--bind".into(), "x".repeat(bytes)]);
            remaining -= bytes;
        }
        let launch = SessionLaunch::new(full.clone()).unwrap();
        assert!(launch.retry_from(start).is_err());
        assert_eq!(launch.args(), full);
        let base = format!("{}.bkr", "a".repeat(MAX_VALUE_BYTES - 4));
        let launch = SessionLaunch::new(vec![
            "--chart".into(),
            "a".into(),
            "--record-replay".into(),
            base.clone(),
        ])
        .unwrap();
        assert!(launch.retry_from(start).is_err());
        assert_eq!(launch.args()[3], base);
    }
    #[test]
    fn retry_preserves_the_pinned_chart_roster_and_native_options() {
        let original = args(&[
            "--chart",
            "songs/a.bms",
            "--local-player",
            "7:100",
            "--local-player",
            "99:200",
            "--buffer-frames",
            "512",
            "--ghost-other",
            "past.bkr",
        ]);
        let first = SessionLaunch::new(original.clone()).unwrap();
        let second = first.retry().unwrap();
        assert_eq!(first.args(), original);
        assert_eq!(second.args(), original);
        assert_eq!(first.attempt(), 0);
        assert_eq!(second.attempt(), 1);
    }
    #[test]
    fn repeated_recordings_derive_distinct_paths_from_original_stem() {
        let first = SessionLaunch::new(args(&[
            "--chart",
            "a.bms",
            "--record-replay",
            "records/my.take.bkr",
        ]))
        .unwrap();
        let second = first.retry().unwrap();
        let third = second.retry().unwrap();
        assert_eq!(second.args()[3], "records/my.take.retry1.bkr");
        assert_eq!(third.args()[3], "records/my.take.retry2.bkr");
        assert_eq!(first.args()[3], "records/my.take.bkr");
    }
    #[test]
    fn malformed_invocations_and_exhaustion_fail_without_mutation() {
        for bad in [
            args(&[]),
            args(&["--chart"]),
            args(&["--chart", ""]),
            args(&["--chart", "a", "--chart", "b"]),
            args(&[
                "--chart",
                "a",
                "--record-replay",
                "x",
                "--record-replay",
                "y",
            ]),
            args(&["--chart", "a\n"]),
        ] {
            assert!(SessionLaunch::new(bad).is_err());
        }
        let mut launch = SessionLaunch::new(args(&["--chart", "a"])).unwrap();
        launch.attempt = u32::MAX;
        assert!(launch.retry().is_err());
        assert_eq!(launch.attempt(), u32::MAX);
        let too_long = format!("{}.bkr", "a".repeat(MAX_VALUE_BYTES - 4));
        let launch = SessionLaunch::new(vec![
            "--chart".into(),
            "a".into(),
            "--record-replay".into(),
            too_long.clone(),
        ])
        .unwrap();
        assert!(launch.retry().is_err());
        assert_eq!(launch.args()[3], too_long);
        let directory =
            SessionLaunch::new(args(&["--chart", "a", "--record-replay", "/"])).unwrap();
        assert!(directory.retry().is_err());
        let directory =
            SessionLaunch::new(args(&["--chart", "a", "--record-replay", "records/"])).unwrap();
        assert!(directory.retry().is_err());
    }
}
