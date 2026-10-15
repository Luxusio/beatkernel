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
/// Keeps the scalar whole-block interface compatible. Unsupported partial delivery
/// remains unavailable; callers that need the refusal use the checked companion.
pub fn asio_pause_observation(
    observation: Option<beatkernel_platform::audio::asio::AsioPresentationObservation>,
    now: beatkernel::time::ClockPoint,
) -> crate::live_pause::LivePauseObservation {
    checked_asio_pause_observation(observation, now).unwrap_or(
        crate::live_pause::LivePauseObservation::Interval {
            observation: None,
            now,
        },
    )
}

/// Copies only complete original renders into the scalar pause interval model.
/// A prefix starting at the original block start is still a partial delivery.
pub fn checked_asio_pause_observation(
    observation: Option<beatkernel_platform::audio::asio::AsioPresentationObservation>,
    now: beatkernel::time::ClockPoint,
) -> Result<
    crate::live_pause::LivePauseObservation,
    beatkernel_platform::audio::asio::AsioPresentationError,
> {
    use beatkernel_platform::audio::asio::{AsioPresentationError, AsioPresentationObservation};
    let observation = observation
        .map(|value| {
            if value.delivered_first_frame != value.render.start_frame
                || value.delivered_frames != value.render.frames
            {
                return Err(AsioPresentationError::Malformed);
            }
            let validated = AsioPresentationObservation::from_delivery(
                value.render,
                value.delivered_first_frame,
                value.delivered_frames,
                value.sample_rate,
                value.host,
                0,
                0,
                value.output_origin,
            )?;
            if validated.output != value.output {
                return Err(AsioPresentationError::Malformed);
            }
            Ok(crate::playback_pause::PauseIntervalObservation {
                output_origin: value.output_origin,
                sample_rate: value.sample_rate,
                render: value.render,
                clock: crate::native_start::StartInterval {
                    output: value.output,
                    before: value.host.before,
                    after: value.host.after,
                },
            })
        })
        .transpose()?;
    Ok(crate::live_pause::LivePauseObservation::Interval { observation, now })
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

/// Qualifies generated Source facts from one original WASAPI publication.
/// Submission remains a delivery bound, not audible presentation or a pause ACK.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn qualify_wasapi_source_packet(
    epoch: u64,
    basis: OutputFrameBasis,
    identity: crate::gameplay::output::ports::SourcePlaybackIdentity,
    query: beatkernel::audio::InterpolatedTargetTime,
    snapshot: beatkernel_platform::audio::SourcePlaybackSnapshot,
) -> Result<
    crate::gameplay::output::ports::NativeSourcePlaybackQuery<AudioStreamSnapshot>,
    crate::gameplay::output::ports::SourcePlaybackPacketError,
> {
    use crate::gameplay::output::ports::{
        NativeSourcePlaybackQuery, SourcePlaybackFacts, SourcePlaybackPacketError,
    };
    if snapshot.revision == 0 {
        return Err(SourcePlaybackPacketError::InvalidRevision);
    }
    if !snapshot.snapshot.telemetry_available {
        return Err(SourcePlaybackPacketError::MissingNativeEvidence);
    }
    let submitted_source_end = basis
        .start_physical_frame()
        .checked_add(snapshot.snapshot.counters.submitted_frames)
        .ok_or(SourcePlaybackPacketError::Overflow)?;
    NativeSourcePlaybackQuery::new(
        epoch,
        basis,
        identity,
        query,
        SourcePlaybackFacts {
            playback: snapshot.playback,
            pulled_source_end: snapshot.pulled_source_end,
            submitted_source_end,
            render: snapshot.snapshot.render,
            revision: snapshot.revision,
        },
        Some(snapshot.snapshot),
    )
}

/// Requires the complete original candidate to match the compound Source read.
/// A changed publication waits; corrupt provenance or packet headers refuse.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn qualify_wasapi_source_observation_packet(
    candidate: crate::native_output_coordinate::CoordinateNativeAudioSnapshot,
    identity: crate::gameplay::output::ports::SourcePlaybackIdentity,
    packet: crate::gameplay::output::ports::NativeSourcePlaybackQuery<AudioStreamSnapshot>,
) -> Result<
    Option<crate::gameplay::output::ports::NativePlaybackObservation<AudioStreamSnapshot>>,
    crate::gameplay::output::ports::SourcePlaybackPacketError,
> {
    use crate::gameplay::output::ports::{
        NativePlaybackObservation, SourcePlaybackPacketError as Error,
    };
    let original = candidate.snapshot();
    if packet.epoch() != original.epoch {
        return Err(Error::EpochMismatch);
    }
    if packet.basis() != original.basis || packet.identity() != identity {
        return Err(Error::BasisMismatch);
    }
    let beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Wasapi {
        snapshot: native, basis,
    } = original.evidence else {
        return Err(Error::CoordinateMismatch);
    };
    if basis != Some(original.basis) {
        return Err(Error::BasisMismatch);
    }
    let query = candidate.source_query().map_err(|_| Error::Overflow)?;
    if !packet.query().compare_interpolated_time(query).is_eq() {
        return Err(Error::CoordinateMismatch);
    }
    if packet.native_evidence().is_none() {
        return Err(Error::MissingNativeEvidence);
    }
    if packet.native_evidence() != Some(&native) {
        return Ok(None);
    }
    NativePlaybackObservation::new(candidate, packet).map(Some)
}

#[cfg(test)]
#[path = "wasapi_source_packet_fixtures.rs"]
mod wasapi_source_packet_fixtures;
