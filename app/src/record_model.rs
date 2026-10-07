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
