//! Portable admission of original WASAPI replacement observations.
use beatkernel::audio::OutputFrameBasis;
use beatkernel_platform::audio::{
    AudioStreamSnapshot, AudioStreamStatus,
    presentation::{
        PresentationError,
        discipline::{DisciplineError, PresentationDiscipline},
    },
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementObservationError {
    EpochMismatch,
    Status(AudioStreamStatus),
    Discipline(DisciplineError),
}
impl std::fmt::Display for ReplacementObservationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replacement observation: {self:?}")
    }
}
impl std::error::Error for ReplacementObservationError {}
/// Epoch refusal precedes all snapshot interpretation. Missing initial evidence
/// waits; inconsistent available metadata and terminal stream states refuse.
pub fn observe_wasapi(
    presentation: &mut PresentationDiscipline,
    epoch: u64,
    snapshot: AudioStreamSnapshot,
    basis: OutputFrameBasis,
) -> Result<bool, ReplacementObservationError> {
    if presentation.epoch() != epoch {
        return Err(ReplacementObservationError::EpochMismatch);
    }
    match snapshot.status {
        AudioStreamStatus::Ready => return Ok(false),
        AudioStreamStatus::Running => {}
        status => return Err(ReplacementObservationError::Status(status)),
    }
    match presentation.observe_with_basis_in_epoch(epoch, snapshot, basis) {
        Ok(_) => Ok(true),
        Err(DisciplineError::Presentation(
            PresentationError::Unavailable | PresentationError::BeforePresentation,
        )) => Ok(false),
        Err(error) => Err(ReplacementObservationError::Discipline(error)),
    }
}
#[cfg(test)]
#[path = "native_replacement_observation_fixtures.rs"]
mod fixtures;
