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
#[path = "observation_fixtures.rs"]
mod fixtures;

/// Admits only original ASIO rendered-block interval evidence from this epoch.
pub fn observe_asio(
    presentation: &mut PresentationDiscipline,
    epoch: u64,
    observation: Option<beatkernel_platform::audio::asio::AsioPresentationObservation>,
) -> Result<bool, ReplacementObservationError> {
    if presentation.epoch() != epoch {
        return Err(ReplacementObservationError::EpochMismatch);
    }
    let Some(observation) = observation else {
        return Ok(false);
    };
    presentation
        .observe_asio_in_epoch(epoch, observation)
        .map(|admission| {
            matches!(
                admission,
                beatkernel::time::presentation::ObservationAdmission::Retained
                    | beatkernel::time::presentation::ObservationAdmission::Progress
            )
        })
        .map_err(ReplacementObservationError::Discipline)
}
/// Copies the complete original render and host interval without substituting receipt time.
pub fn asio_pause_observation(
    observation: Option<beatkernel_platform::audio::asio::AsioPresentationObservation>,
    now: beatkernel::time::ClockPoint,
) -> crate::live_pause::LivePauseObservation {
    crate::live_pause::LivePauseObservation::Interval {
        observation: observation.map(|value| crate::playback_pause::PauseIntervalObservation {
            output_origin: value.output_origin,
            sample_rate: value.sample_rate,
            render: value.render,
            clock: crate::native_start::StartInterval {
                output: value.output,
                before: value.host.before,
                after: value.host.after,
            },
        }),
        now,
    }
}
#[cfg(test)]
#[path = "asio_observation_fixtures.rs"]
mod asio_fixtures;

/// Admits absolute ASIO evidence against the newly captured native frame basis.
pub fn observe_asio_with_basis(
    presentation: &mut PresentationDiscipline,
    epoch: u64,
    observation: Option<beatkernel_platform::audio::asio::AsioPresentationObservation>,
    basis: OutputFrameBasis,
) -> Result<bool, ReplacementObservationError> {
    if presentation.epoch() != epoch {
        return Err(ReplacementObservationError::EpochMismatch);
    }
    let Some(observation) = observation else {
        return Ok(false);
    };
    presentation
        .observe_asio_with_basis_in_epoch(epoch, observation, basis)
        .map(|admission| {
            matches!(
                admission,
                beatkernel::time::presentation::ObservationAdmission::Retained
                    | beatkernel::time::presentation::ObservationAdmission::Progress
            )
        })
        .map_err(ReplacementObservationError::Discipline)
}
