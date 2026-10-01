//! Immutable native invocations for fresh graphical sessions, without device ownership.
use crate::settings::{MAX_FIELDS, MAX_TOTAL_BYTES, MAX_VALUE_BYTES};
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
