//! Bounded saved-record discovery and logical prefix inspection on a metadata worker.
use crate::{
    competition::ScoreSummary,
    competition_live::{load_chart_with_seed, replay_limits},
    replay_playback::{decode_section_setup, read_replay, reconstruct_section, RecordedSetup},
    settings::{MAX_VALUE_BYTES, NativeSettings},
};
use beatkernel::{
    judge::{JudgeGrade, JudgeProfile, JudgeWindow},
    replay::codec::ReplayFile,
    time::{Duration, Timestamp},
};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

const MAX_INSPECTED: usize = 4096;
const MAX_RETAINED: usize = 256;

pub use crate::record_model::{RecordCatalog, RecordPreview};
impl RecordCatalog {
    pub fn scan(directory: &Path) -> Result<Self, String> {
        settings_path(directory)?;
        let mut catalog = Self {
            entries: Vec::new(),
            truncated: false,
        };
        for (index, entry) in fs::read_dir(directory)
            .map_err(|error| error.to_string())?
            .enumerate()
        {
            let entry = entry.map_err(|error| error.to_string())?;
            if index == MAX_INSPECTED {
                catalog.truncated = true;
                break;
            }
            let path = entry.path();
            if is_record_path(&path)
                && entry
                    .file_type()
                    .map_err(|error| error.to_string())?
                    .is_file()
            {
                catalog.admit(path)?;
            }
        }
        Ok(catalog)
    }
    fn admit(&mut self, path: PathBuf) -> Result<(), String> {
        settings_path(&path)?;
        let index = self
            .entries
            .binary_search(&path)
            .unwrap_or_else(|index| index);
        if self.entries.len() == MAX_RETAINED {
            self.truncated = true;
            if index >= MAX_RETAINED {
                return Ok(());
            }
            self.entries.pop();
        }
        self.entries
            .try_reserve(1)
            .map_err(|_| "record catalog allocation failed")?;
        self.entries.insert(index, path);
        Ok(())
    }
}

impl RecordPreview {
    pub fn inspect(path: &Path, chart: &Path, settings: &NativeSettings) -> Result<Self, String> {
        settings_path(path)?;
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("saved record must be a regular non-symlink file".into());
        }
        let limits = replay_limits().map_err(|error| error.to_string())?;
        let mut reader = File::open(path).map_err(|error| error.to_string())?;
        if !reader
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("saved record must be a regular file".into());
        }
        let file = read_replay(&mut reader, limits).map_err(|error| error.to_string())?;
        let setup =
            decode_section_setup(&file.header.options).map_err(|error| error.to_string())?;
        let source =
            load_chart_with_seed(chart, setup.chart_seed).map_err(|error| error.to_string())?;
        let header = file.header.clone();
        let preview = Self::from_file(path, &source, settings, file)?;
        #[cfg(not(target_arch = "wasm32"))]
        let preview = {
            let mut preview = preview;
            match crate::native_result_archive::read_sidecar(path) {
                Ok(None) => {}
                Ok(Some(bytes)) => match crate::result_archive::decode_archive(&bytes) {
                    Ok(archive) => preview.attach_archive(&header, &archive, None),
                    Err(error) => preview.archive_error = Some(error.to_string()),
                },
                Err(error) => preview.archive_error = Some(error.to_string()),
            }
            preview
        };
        #[cfg(target_arch = "wasm32")]
        let _ = header;
        Ok(preview)
    }
    fn from_file(
        path: &Path,
        source: &beatkernel_bms::BmsChart,
        settings: &NativeSettings,
        file: ReplayFile,
    ) -> Result<Self, String> {
        let setup =
            decode_section_setup(&file.header.options).map_err(|error| error.to_string())?;
        let expected = draft_section(settings)?;
        if setup.chart_seed != expected.chart_seed {
            return Err("saved record chart seed differs from the current draft".into());
        }
        if setup.profile != expected.profile
            || setup.start != expected.start
            || setup.end != expected.end
            || setup.input_mode != expected.input_mode
        {
            return Err("saved record profile or section differs from the current draft".into());
        }
        let replay = reconstruct_section(
            source,
            file,
            replay_limits().map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut score = ScoreSummary::default();
        score
            .observe(replay.results())
            .map_err(|error| error.to_string())?;
        Ok(Self {
            path: path.into(),
            records: replay.records().len(),
            recorded_until: replay.records().last().map(|record| record.song_time),
            start: setup.start,
            end: setup.end,
            historical: None,
            historical_score: None,
            archive_error: None,
            score,
        })
    }
    /// Explicit decoded archive association; failed association keeps a valid prefix.
    pub fn from_file_with_archive(
        path: &Path,
        source: &beatkernel_bms::BmsChart,
        settings: &NativeSettings,
        file: ReplayFile,
        archive: Option<&crate::result_archive::ResultArchive>,
        player: Option<crate::local_players::PlayerId>,
    ) -> Result<Self, String> {
        let association = archive.map(|archive| prepare_association(archive, &file.header, player));
        let mut preview = Self::from_file(path, source, settings, file)?;
        match association {
            Some(Ok((historical, score))) => {
                preview.historical = Some(historical);
                preview.historical_score = score;
            }
            Some(Err(error)) => preview.archive_error = Some(error.to_string()),
            None if player.is_some() => {
                preview.archive_error = Some("historical player requires an archive".into())
            }
            None => {}
        }
        Ok(preview)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn attach_archive(
        &mut self,
        header: &beatkernel::replay::ReplayHeader,
        archive: &crate::result_archive::ResultArchive,
        player: Option<crate::local_players::PlayerId>,
    ) {
        match prepare_association(archive, header, player) {
            Ok((historical, score)) => {
                self.historical = Some(historical);
                self.historical_score = score;
                self.archive_error = None;
            }
            Err(error) => {
                self.historical = None;
                self.historical_score = None;
                self.archive_error = Some(error.to_string());
            }
        }
    }
}
fn prepare_association(
    archive: &crate::result_archive::ResultArchive,
    header: &beatkernel::replay::ReplayHeader,
    player: Option<crate::local_players::PlayerId>,
) -> Result<
    (
        crate::record_model::HistoricalRecordValue,
        Option<std::sync::Arc<crate::result_archive::ArchivedScore>>,
    ),
    String,
> {
    let entry = crate::record_association::associate(archive, header, player)
        .map_err(|error| error.to_string())?;
    let score = entry
        .score
        .as_ref()
        .map(crate::result_archive::ArchivedScore::try_copy)
        .transpose()
        .map_err(|error| error.to_string())?
        .map(std::sync::Arc::new);
    Ok(((entry.player, entry.result), score))
}
fn is_record_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bkr"))
}
fn settings_path(path: &Path) -> Result<(), String> {
    let text = path.to_str().ok_or("record path must be UTF-8")?;
    if text.is_empty()
        || text.len() > MAX_VALUE_BYTES
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err("record path requires 1..4096 UTF-8 bytes without controls".into());
    }
    Ok(())
}
fn draft_setup(settings: &NativeSettings) -> Result<(JudgeProfile, Timestamp), String> {
    let value = |flag| {
        settings
            .fields()
            .iter()
            .find(|row| row.flag == flag)
            .map(|row| row.value.as_str())
            .unwrap_or("")
    };
    let nonnegative = |flag, default: i64| -> Result<i64, String> {
        let text = value(flag);
        if text.is_empty() {
            return Ok(default);
        }
        if flag == "--start-ns" && !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!("{flag} requires nonnegative decimal nanoseconds"));
        }
        let value = text
            .parse::<i64>()
            .map_err(|_| format!("{flag} requires i64 nanoseconds"))?;
        if value < 0 {
            return Err(format!("{flag} must be nonnegative"));
        }
        Ok(value)
    };
    let offset = if value("--input-offset-ns").is_empty() {
        0
    } else {
        value("--input-offset-ns")
            .parse::<i64>()
            .map_err(|_| "input-offset-ns must be signed i64 nanoseconds")?
    };
    let profile = JudgeProfile::new(
        vec![JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(nonnegative("--early-ns", 150_000_000)?),
            late: Duration::from_nanos(nonnegative("--late-ns", 150_000_000)?),
        }],
        Duration::from_nanos(offset),
    )
    .map_err(|error| error.to_string())?;
    Ok((
        profile,
        Timestamp::from_nanos(nonnegative("--start-ns", 0)?),
    ))
}

/// Full current native draft identity; finite endpoints are strictly after the start.
fn draft_section(settings: &NativeSettings) -> Result<RecordedSetup, String> {
    let (profile, start) = draft_setup(settings)?;
    let text = settings
        .fields()
        .iter()
        .find(|row| row.flag == "--end-ns")
        .map(|row| row.value.as_str())
        .unwrap_or("");
    let end = if text.is_empty() {
        None
    } else {
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("--end-ns requires nonnegative decimal nanoseconds".into());
        }
        let end = Timestamp::from_nanos(
            text.parse::<i64>()
                .map_err(|_| "--end-ns requires i64 nanoseconds")?,
        );
        if end <= start {
            return Err("--end-ns must be after the section start".into());
        }
        Some(end)
    };
    Ok(RecordedSetup {
        profile,
        start,
        end,
        chart_seed: settings.chart_seed()?,
        input_mode: beatkernel_bms::BmsInputMode::ButtonOnly,
    })
}
#[cfg(test)]
#[path = "record_catalog_section_fixtures.rs"]
pub(crate) mod section_fixtures;

#[cfg(test)]
mod fixtures {
    use super::*;
    use crate::{replay_capture::LiveReplayCapture, settings::SettingsHost};
    use beatkernel::{
        judge::JudgeEngine,
        replay::{ReplayOperation, ReplayRecord},
        time::ClockDomainId,
    };
    use beatkernel_bms::{ParseOptions, parse};
    fn settings(args: &[&str]) -> NativeSettings {
        NativeSettings::from_args(
            &args.iter().map(|text| (*text).into()).collect::<Vec<_>>(),
            SettingsHost::Linux,
        )
        .unwrap()
    }
    fn source() -> beatkernel_bms::BmsChart {
        parse(
            "#BPM 120\n#WAV01 head.wav\n#00011:01\n#00112:01\n",
            ParseOptions::default(),
        )
        .unwrap()
    }
    fn recording(settings: &NativeSettings, times: &[i64]) -> ReplayFile {
        let source = source();
        let (profile, start) = draft_setup(settings).unwrap();
        let selected = crate::section_start::source_at(&source, start).unwrap();
        let judge =
            JudgeEngine::new(selected.compile().unwrap().chart, selected.rules(), profile).unwrap();
        let mut file = LiveReplayCapture::new_at_with_chart_seed(
            &judge,
            ClockDomainId(17),
            replay_limits().unwrap(),
            start,
            settings.chart_seed().unwrap(),
        )
        .unwrap()
        .into_file();
        file.records = times
            .iter()
            .enumerate()
            .map(|(index, time)| ReplayRecord {
                ordinal: index as u64,
                song_time: Timestamp::from_nanos(*time),
                operation: ReplayOperation::Advance,
            })
            .collect();
        file
    }
    #[test]
    fn record_comparison_requires_the_draft_branch_seed() {
        let draft = settings(&["--chart-seed", "3"]);
        let file = recording(&draft, &[3_000_000_000]);
        assert!(
            RecordPreview::from_file(Path::new("record.bkr"), &source(), &draft, file.clone())
                .is_ok()
        );
        assert!(
            RecordPreview::from_file(
                Path::new("record.bkr"),
                &source(),
                &settings(&[]),
                file.clone()
            )
            .unwrap_err()
            .contains("chart seed")
        );
        assert!(
            RecordPreview::from_file(
                Path::new("record.bkr"),
                &source(),
                &settings(&["--chart-seed", "18446744073709551615"]),
                file
            )
            .is_err()
        );
    }
    #[test]
    fn bounded_catalog_is_sorted_marks_overflow_and_rejects_unusable_paths() {
        let mut catalog = RecordCatalog {
            entries: Vec::new(),
            truncated: false,
        };
        for index in (0..300).rev() {
            catalog
                .admit(PathBuf::from(format!("records/{index:04}.bkr")))
                .unwrap();
        }
        assert_eq!(catalog.entries.len(), 256);
        assert!(catalog.truncated);
        assert_eq!(catalog.entries[0], PathBuf::from("records/0000.bkr"));
        assert_eq!(catalog.entries[255], PathBuf::from("records/0255.bkr"));
        assert!(catalog.entries.windows(2).all(|pair| pair[0] < pair[1]));
        for path in ["a.bkr", "a.BKR", "a.BkR"] {
            assert!(is_record_path(Path::new(path)));
        }
        for path in ["a.bkr.tmp", "a.txt", "no-extension"] {
            assert!(!is_record_path(Path::new(path)));
        }
        for path in [
            "".to_owned(),
            "bad\n.bkr".into(),
            "x".repeat(MAX_VALUE_BYTES + 1),
        ] {
            assert!(settings_path(Path::new(&path)).is_err());
        }
        assert!(settings_path(Path::new(&"x".repeat(MAX_VALUE_BYTES))).is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            assert!(settings_path(Path::new(&std::ffi::OsString::from_vec(vec![0xff]))).is_err());
        }
    }
    #[test]
    fn preview_scores_actual_prefix_only_and_handles_empty_and_section_logs() {
        let draft = settings(&[]);
        let empty = RecordPreview::from_file(
            Path::new("empty.bkr"),
            &source(),
            &draft,
            recording(&draft, &[]),
        )
        .unwrap();
        assert_eq!(empty.records, 0);
        assert_eq!(empty.recorded_until, None);
        assert_eq!(empty.score, ScoreSummary::default());
        let prefix = RecordPreview::from_file(
            Path::new("prefix.bkr"),
            &source(),
            &draft,
            recording(&draft, &[200_000_000]),
        )
        .unwrap();
        assert_eq!(prefix.records, 1);
        assert_eq!(
            prefix.recorded_until,
            Some(Timestamp::from_nanos(200_000_000))
        );
        assert_eq!(prefix.score.misses, 1); // Future note is not synthetically advanced.
        assert_eq!(prefix.score.hits, 0);
        let section = settings(&[
            "--start-ns",
            "1",
            "--input-offset-ns",
            "-19",
            "--early-ns",
            "11",
            "--late-ns",
            "23",
        ]);
        let preview = RecordPreview::from_file(
            Path::new("section.bkr"),
            &source(),
            &section,
            recording(&section, &[2_000_000_043]),
        )
        .unwrap();
        assert_eq!(preview.start, Timestamp::from_nanos(1));
        assert_eq!(preview.score.misses, 1);
    }
    #[test]
    fn draft_profile_section_source_and_invalid_metadata_must_match() {
        let draft = settings(&[]);
        let file = recording(&draft, &[]);
        for args in [
            vec!["--start-ns", "1"],
            vec!["--early-ns", "1"],
            vec!["--late-ns", "1"],
            vec!["--input-offset-ns", "1"],
        ] {
            assert!(
                RecordPreview::from_file(
                    Path::new("record.bkr"),
                    &source(),
                    &settings(&args),
                    file.clone()
                )
                .is_err()
            );
        }
        let changed = parse(
            "#BPM 121\n#WAV01 head.wav\n#00011:01\n#00112:01\n",
            ParseOptions::default(),
        )
        .unwrap();
        assert!(
            RecordPreview::from_file(Path::new("record.bkr"), &changed, &draft, file.clone())
                .is_err()
        );
        let mut malformed = file;
        malformed.header.options.push(0);
        assert!(
            RecordPreview::from_file(Path::new("record.bkr"), &source(), &draft, malformed)
                .is_err()
        );
        for args in [
            vec!["--start-ns", "-1"],
            vec!["--early-ns", "-1"],
            vec!["--late-ns", "9223372036854775808"],
            vec!["--input-offset-ns", "bad"],
        ] {
            assert!(draft_setup(&settings(&args)).is_err());
        }
        assert_eq!(
            draft_setup(&settings(&[
                "--early-ns",
                "+150000000",
                "--late-ns",
                "+150000000"
            ]))
            .unwrap(),
            draft_setup(&draft).unwrap()
        );
        assert!(draft_setup(&settings(&["--start-ns", "+1"])).is_err());
        for host in [
            SettingsHost::Windows,
            SettingsHost::Linux,
            SettingsHost::Macos,
        ] {
            assert_eq!(
                draft_setup(&NativeSettings::from_args(&[], host).unwrap()).unwrap(),
                draft_setup(&draft).unwrap()
            );
        }
    }
}

#[cfg(test)]
#[path = "record_stored_score_fixtures.rs"]
mod record_stored_score_fixtures;
