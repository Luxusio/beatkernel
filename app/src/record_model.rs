//! Pure recording catalog and historical preview values; acquisition stays outside UI.
use crate::{competition::ScoreSummary, local_players::PlayerId, result_archive::ArchivedResult};
use beatkernel::time::Timestamp;
use std::path::PathBuf;

pub type HistoricalRecordValue = (PlayerId, ArchivedResult);
/// Direct recording paths retained by a bounded catalog worker.
#[derive(Clone, Debug)]
pub struct RecordCatalog {
    pub entries: Vec<PathBuf>,
    pub truncated: bool,
}
/// Reconstructed accepted-prefix statistics and separately associated historical data.
#[derive(Clone, Debug)]
pub struct RecordPreview {
    pub bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    pub historical_bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    pub path: PathBuf,
    pub records: usize,
    pub recorded_until: Option<Timestamp>,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub historical: Option<HistoricalRecordValue>,
    pub historical_comparison:
        Option<std::sync::Arc<Option<crate::competition_presentation::CompetitionSnapshot>>>,
    pub historical_score: Option<std::sync::Arc<crate::result_archive::ArchivedScore>>,
    pub archive_error: Option<String>,
    pub score: ScoreSummary,
}

/// Display-only accepted-prefix values and independently associated stored data.
/// Contains no judge, replay cursor, completion proof or mutable timing accumulator.
#[derive(Clone, Debug)]
pub struct FrozenRecordPreview {
    pub bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    pub historical_bms_score: Option<crate::judgment_policy::BmsScoreSummary>,
    pub path: PathBuf,
    pub records: usize,
    pub recorded_until: Option<Timestamp>,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub historical: Option<HistoricalRecordValue>,
    pub historical_comparison:
        Option<std::sync::Arc<Option<crate::competition_presentation::CompetitionSnapshot>>>,
    pub historical_score: Option<std::sync::Arc<crate::result_archive::ArchivedScore>>,
    pub archive_error: Option<String>,
    pub score: crate::result_archive::ArchivedScore,
}
impl FrozenRecordPreview {
    pub fn from_record(value: &RecordPreview) -> Result<Self, String> {
        let frozen = Self {
            bms_score: value.bms_score,
            historical_bms_score: value.historical_bms_score,
            path: value.path.clone(),
            records: value.records,
            recorded_until: value.recorded_until,
            start: value.start,
            end: value.end,
            historical: value.historical,
            historical_comparison: value.historical_comparison.clone(),
            historical_score: value.historical_score.clone(),
            archive_error: value.archive_error.clone(),
            score: crate::result_archive::ArchivedScore::from_summary(&value.score)
                .map_err(|error| error.to_string())?,
        };
        frozen.validate()?;
        Ok(frozen)
    }
    pub fn validate(&self) -> Result<(), String> {
        self.score.validate().map_err(|error| error.to_string())?;
        if self.path.as_os_str().is_empty()
            || self.path.to_string_lossy().len() > crate::settings::MAX_VALUE_BYTES
            || self.path.to_string_lossy().chars().any(char::is_control)
            || self.records > 1_000_000
            || (self.records == 0) != self.recorded_until.is_none()
            || self.start < Timestamp::ZERO
            || self.end.is_some_and(|end| end <= self.start)
        {
            return Err("frozen record preview exceeds path/prefix bounds".into());
        }
        if let Some(classes) = self.bms_score {
            classes
                .validate_for(self.score.hits, self.score.misses)
                .map_err(|error| error.to_string())?;
        }
        if let Some(value) = self.historical {
            if value.0 .0 == 0 {
                return Err("frozen historical player is zero".into());
            }
            crate::result_archive::validate_frozen_result(&value.1)?;
        } else if self.historical_score.is_some()
            || self.historical_comparison.is_some()
            || self.historical_bms_score.is_some()
        {
            return Err("frozen stored metadata requires associated historical value".into());
        }
        if let Some(score) = &self.historical_score {
            score.validate().map_err(|error| error.to_string())?;
        }
        if let Some(classes) = self.historical_bms_score {
            let score = self
                .historical_score
                .as_ref()
                .ok_or("frozen stored class score requires historical counts")?;
            classes
                .validate_for(score.hits, score.misses)
                .map_err(|error| error.to_string())?;
        }
        if let Some(Some(snapshot)) = self.historical_comparison.as_deref() {
            crate::result_archive::validate_visual_comparison(snapshot)?;
        }
        if self
            .archive_error
            .as_ref()
            .is_some_and(|error| error.len() > crate::settings::MAX_VALUE_BYTES)
        {
            return Err("frozen record archive diagnostic exceeds limit".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "record_model_visual_fixtures.rs"]
pub(crate) mod visual_fixtures;
