//! Static effect contracts for gameplay output.
use super::domain::control::{OutputCapability, OutputReply, OutputRequest};
use crate::{gameplay_presentation::GameplayPresentationPort, live_pause::LivePauseObservation};
use beatkernel::{
    audio::{
        Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, SoftwareOutputState,
        StoppedMixerSource,
    },
    time::{ClockPair, ClockPoint},
};
use std::io;

#[cfg(test)]
#[path = "source_playback_packet_fixtures.rs"]
mod source_playback_packet_fixtures;

#[cfg(test)]
#[path = "source_conditional_retirement_packet_fixtures.rs"]
pub(crate) mod source_conditional_retirement_packet_fixtures;

#[cfg(test)]
#[path = "target_playback_packet_fixtures.rs"]
mod target_playback_packet_fixtures;

/// Original compound converted publication; the two revisions have distinct owners.
#[derive(Clone, Copy, Debug)]
pub struct NativeTargetPlaybackFacts {
    pub playback: crate::native_playback_phase::ConvertedPlaybackFacts,
    pub source: Option<RenderReport>,
    pub converted: Option<beatkernel::audio::ConvertedRenderReport>,
    pub boundaries: beatkernel::audio::ConvertedBoundaryFacts,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetPlaybackPacketError {
    InvalidRevision,
    InvalidMapping(crate::native_playback_phase::NativePlaybackPhaseError),
    InvalidReport,
    InvalidBoundary,
    MissingNativeEvidence,
    EpochMismatch,
    BasisMismatch,
    CoordinateMismatch,
    Overflow,
}
impl std::fmt::Display for TargetPlaybackPacketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "target playback packet: {self:?}")
    }
}
impl std::error::Error for TargetPlaybackPacketError {}

/// Mathematical correspondence with untouched adapter evidence, without native admission.
#[derive(Clone, Copy, Debug)]
pub struct NativeTargetPlaybackQuery<E> {
    epoch: u64,
    basis: beatkernel::audio::TargetFrameBasis,
    identity: SourcePlaybackIdentity,
    query: beatkernel::audio::InterpolatedTargetTime,
    facts: NativeTargetPlaybackFacts,
    native_evidence: Option<E>,
}
impl<E> NativeTargetPlaybackQuery<E> {
    pub fn new(
        epoch: u64,
        basis: beatkernel::audio::TargetFrameBasis,
        identity: SourcePlaybackIdentity,
        query: beatkernel::audio::InterpolatedTargetTime,
        facts: NativeTargetPlaybackFacts,
        native_evidence: Option<E>,
    ) -> Result<Self, TargetPlaybackPacketError> {
        use TargetPlaybackPacketError as Error;
        if facts.revision & 1 != 0 || facts.revision == u64::MAX {
            return Err(Error::InvalidRevision);
        }
        crate::native_playback_phase::NativePlaybackMapping::converted(
            epoch,
            basis,
            identity,
            query,
            facts.playback,
        )
        .map_err(Error::InvalidMapping)?;
        let check_source = |report: RenderReport, pulled: u64| -> Result<(), Error> {
            let frames = u64::try_from(report.frames).map_err(|_| Error::InvalidReport)?;
            let playback =
                u64::try_from(report.playback_frames).map_err(|_| Error::InvalidReport)?;
            if report.playback_start_frame > report.start_frame
                || playback > frames
                || !report
                    .start_frame
                    .checked_add(frames)
                    .is_some_and(|end| end <= pulled)
                || report.playback_start_frame.checked_add(playback).is_none()
                || report
                    .playback_end_physical_frame
                    .is_some_and(|end| end > pulled)
            {
                return Err(Error::InvalidReport);
            }
            Ok(())
        };
        if let Some(source) = facts.source {
            check_source(source, facts.playback.pulled_source_end)?;
        }
        if let Some(report) = facts.converted {
            use beatkernel::audio::ConvertedOutputState;
            let frames = u64::try_from(report.target_frames).map_err(|_| Error::InvalidReport)?;
            let start = report.source_start_position;
            let end = report.source_position;
            let proper = |p: beatkernel::audio::SourcePosition| {
                p.denominator != 0 && p.numerator < p.denominator
            };
            if report.source_rate != identity.hz()
                || report.target_rate != basis.sample_rate()
                || report.target_frame_cursor < frames
                || report.pulled_source_frame_cursor > facts.playback.pulled_source_end
                || !proper(start)
                || !proper(end)
                || start.frame > end.frame
                || (start.frame == end.frame
                    && u128::from(start.numerator) * u128::from(end.denominator)
                        > u128::from(end.numerator) * u128::from(start.denominator))
                || (report.state == ConvertedOutputState::Held
                    && (report.source.is_some() || start != end))
                || (report.state == ConvertedOutputState::Active && report.source.is_none())
            {
                return Err(Error::InvalidReport);
            }
            let exact_end = report
                .target_start_time
                .checked_add_frames(frames, report.target_rate)
                .map_err(|error| match error {
                    beatkernel::audio::AudioError::Overflow => Error::Overflow,
                    _ => Error::InvalidReport,
                })?;
            let exact = beatkernel::audio::InterpolatedTargetTime::new(exact_end, 0, 0, 1, 0, 1)
                .map_err(|_| Error::InvalidReport)?;
            if !exact.compare_target_time(report.target_end_time).is_eq()
                || exact
                    .compare_target_time(facts.playback.generated_target_end)
                    .is_gt()
            {
                return Err(Error::InvalidReport);
            }
            if let Some(source) = report.source {
                check_source(source, report.pulled_source_frame_cursor)?;
            }
            // Startup gates may still be beyond source lookahead; adoptions cannot.
            for frame in [report.pause_source_frame, report.resume_source_frame]
                .into_iter()
                .flatten()
            {
                if frame > report.pulled_source_frame_cursor {
                    return Err(Error::InvalidReport);
                }
            }
        }
        if facts.boundaries.origin != Some(basis.origin())
            || facts.boundaries.source_rate != identity.hz()
        {
            return Err(Error::InvalidBoundary);
        }
        for boundary in [
            facts.boundaries.startup,
            facts.boundaries.pause,
            facts.boundaries.resume,
            facts.boundaries.end,
        ]
        .into_iter()
        .flatten()
        {
            let exact =
                beatkernel::audio::InterpolatedTargetTime::new(boundary.target_time, 0, 0, 1, 0, 1)
                    .map_err(|_| Error::InvalidBoundary)?;
            if boundary.source_frame > facts.playback.pulled_source_end
                || exact
                    .compare_target_time(facts.playback.generated_target_end)
                    .is_gt()
            {
                return Err(Error::InvalidBoundary);
            }
        }
        Ok(Self {
            epoch,
            basis,
            identity,
            query,
            facts,
            native_evidence,
        })
    }
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
    pub const fn basis(&self) -> beatkernel::audio::TargetFrameBasis {
        self.basis
    }
    pub const fn identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub const fn query(&self) -> beatkernel::audio::InterpolatedTargetTime {
        self.query
    }
    pub const fn facts(&self) -> NativeTargetPlaybackFacts {
        self.facts
    }
    pub const fn native_evidence(&self) -> Option<&E> {
        self.native_evidence.as_ref()
    }
}

/// Exact original Target coordinate and its same-publication mathematical packet.
#[derive(Clone, Copy, Debug)]
pub struct NativeTargetPlaybackObservation<E> {
    coordinate: crate::native_output_coordinate::CoordinateTargetNativeAudioSnapshot,
    playback: NativeTargetPlaybackQuery<E>,
}
impl<E> NativeTargetPlaybackObservation<E> {
    pub fn new(
        coordinate: crate::native_output_coordinate::CoordinateTargetNativeAudioSnapshot,
        playback: NativeTargetPlaybackQuery<E>,
    ) -> Result<Self, TargetPlaybackPacketError> {
        use TargetPlaybackPacketError as Error;
        if playback.native_evidence.is_none() {
            return Err(Error::MissingNativeEvidence);
        }
        let snapshot = coordinate.snapshot();
        if snapshot.epoch != playback.epoch {
            return Err(Error::EpochMismatch);
        }
        if snapshot.basis != playback.basis {
            return Err(Error::BasisMismatch);
        }
        let crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame } =
            coordinate.coordinate()
        else {
            return Err(Error::CoordinateMismatch);
        };
        if !playback
            .query
            .compare_native_time_with_offset(
                playback.basis.start_time(),
                frame,
                u64::from(playback.basis.sample_rate()),
                0,
            )
            .map_err(|_| Error::Overflow)?
            .is_eq()
        {
            return Err(Error::CoordinateMismatch);
        }
        Ok(Self {
            coordinate,
            playback,
        })
    }
    pub const fn coordinate(
        &self,
    ) -> crate::native_output_coordinate::CoordinateTargetNativeAudioSnapshot {
        self.coordinate
    }
    pub const fn playback(&self) -> &NativeTargetPlaybackQuery<E> {
        &self.playback
    }
    pub fn into_playback(self) -> NativeTargetPlaybackQuery<E> {
        self.playback
    }
}

/// Static converted playback contract; independent reads cannot implement the packet.
pub trait TargetPlaybackNativeOutputBackend<O>:
    OriginalCoordinateTargetNativeOutputBackend<O>
{
    type NativeEvidence;
    fn observe_native_target_playback(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackObservation<Self::NativeEvidence>>, Self::Error>;
    fn target_playback_at(
        &self,
        output: &Self::Output,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackQuery<Self::NativeEvidence>>, Self::Error>;
}

/// The worker's conditional decision, separate from completed native retirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetRetirementStatus {
    Pending,
    Refused,
    Cancelled,
    Authorized,
}

/// Static live-output retirement permission with an output-bound opaque ticket.
/// Implementations must not substitute an unconditional native stop.
pub trait ConditionalTargetRetirementNativeOutputBackend<O>:
    TargetPlaybackNativeOutputBackend<O>
{
    type Ticket;
    fn submit_target_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        request: &Self::Request,
    ) -> Result<Option<Self::Ticket>, Self::Error>;
    fn poll_target_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<TargetRetirementStatus, Self::Error>;
    fn cancel_target_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<TargetRetirementStatus, Self::Error>;
}

/// Queues mathematical history retirement on the current statically owned output.
/// A returned revision does not acknowledge native playback or completed retirement.
pub trait TargetHistoryRetirementNativeOutputBackend<O>:
    TargetPlaybackNativeOutputBackend<O>
{
    /// Checks `expected_epoch` and `expected_basis` against the actual current
    /// output before mutating the queue, preserving original Source identity
    /// validation. Busy returns `None`; an accepted revision proves only that
    /// the mathematical request was queued.
    fn request_target_history_retirement(
        &mut self,
        output: &mut Self::Output,
        expected_epoch: u64,
        expected_basis: beatkernel::audio::TargetFrameBasis,
        identity: SourcePlaybackIdentity,
        source_pin: u64,
        target_pin: beatkernel::audio::TargetTime,
    ) -> Result<Option<u64>, Self::Error>;
}

/// Retained source-history identity. This authenticates arithmetic, not acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePlaybackIdentity {
    hz: u32,
    initial_physical: u64,
    initial_playback: u64,
}
impl SourcePlaybackIdentity {
    pub fn new(
        hz: u32,
        initial_physical: u64,
        initial_playback: u64,
    ) -> Result<Self, SourcePlaybackPacketError> {
        if hz == 0 || initial_playback > initial_physical {
            return Err(SourcePlaybackPacketError::InvalidIdentity);
        }
        Ok(Self {
            hz,
            initial_physical,
            initial_playback,
        })
    }
    pub const fn hz(self) -> u32 {
        self.hz
    }
    pub const fn initial_physical(self) -> u64 {
        self.initial_physical
    }
    pub const fn initial_playback(self) -> u64 {
        self.initial_playback
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePlaybackPacketError {
    InvalidIdentity,
    BasisMismatch,
    InvalidRevision,
    InvalidPrefix,
    InvalidSpan,
    InvalidReport,
    QueryOutsidePrefix,
    QueryOutsideSpan,
    MissingNativeEvidence,
    EpochMismatch,
    CoordinateMismatch,
    Overflow,
}
impl std::fmt::Display for SourcePlaybackPacketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source playback packet: {self:?}")
    }
}
impl std::error::Error for SourcePlaybackPacketError {}

/// Same-publication source facts; counters describe software generation/admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePlaybackFacts {
    pub playback: beatkernel::audio::SourcePlaybackSpan,
    pub pulled_source_end: u64,
    pub submitted_source_end: u64,
    pub render: Option<RenderReport>,
    pub revision: u64,
}

/// Checked mathematical packet. Native evidence remains adapter-owned and unmodified.
/// Construction grants neither hardware authenticity nor gameplay permission.
#[derive(Clone, Copy, Debug)]
pub struct NativeSourcePlaybackQuery<E> {
    epoch: u64,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    query: beatkernel::audio::InterpolatedTargetTime,
    facts: SourcePlaybackFacts,
    native_evidence: Option<E>,
}
impl<E> NativeSourcePlaybackQuery<E> {
    pub fn new(
        epoch: u64,
        basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        query: beatkernel::audio::InterpolatedTargetTime,
        facts: SourcePlaybackFacts,
        native_evidence: Option<E>,
    ) -> Result<Self, SourcePlaybackPacketError> {
        use SourcePlaybackPacketError as Error;
        if basis.sample_rate() != identity.hz
            || identity.initial_physical > basis.start_physical_frame()
        {
            return Err(Error::BasisMismatch);
        }
        if facts.revision & 1 != 0 {
            return Err(Error::InvalidRevision);
        }
        if facts.submitted_source_end <= basis.start_physical_frame()
            || facts.submitted_source_end > facts.pulled_source_end
        {
            return Err(Error::InvalidPrefix);
        }
        let span = facts.playback;
        if span.physical_start() < identity.initial_physical
            || span.physical_start() > facts.pulled_source_end
            || span
                .physical_end()
                .is_some_and(|end| end > facts.pulled_source_end)
            || span.playback_start() < identity.initial_playback
            || span.playback_start() - identity.initial_playback
                > span.physical_start() - identity.initial_physical
        {
            return Err(Error::InvalidSpan);
        }
        let compare = |frame| {
            query
                .compare_source_time_frame(frame, identity.hz)
                .map_err(|_| Error::Overflow)
        };
        if compare(basis.start_physical_frame())?.is_lt()
            || compare(facts.submitted_source_end)?.is_gt()
        {
            return Err(Error::QueryOutsidePrefix);
        }
        // The submitted prefix was already checked to lie within the actual pull.
        if compare(span.physical_start())?.is_lt()
            || span
                .physical_end()
                .is_some_and(|end| !compare(end).is_ok_and(|order| order.is_lt()))
        {
            return Err(Error::QueryOutsideSpan);
        }
        if let Some(report) = facts.render {
            let frames = u64::try_from(report.frames).map_err(|_| Error::InvalidReport)?;
            let playback_frames =
                u64::try_from(report.playback_frames).map_err(|_| Error::InvalidReport)?;
            if report.playback_start_frame > report.start_frame
                || playback_frames > frames
                || !report
                    .start_frame
                    .checked_add(frames)
                    .is_some_and(|end| end <= facts.pulled_source_end)
                || report
                    .playback_start_frame
                    .checked_add(playback_frames)
                    .is_none()
                || report
                    .playback_end_physical_frame
                    .is_some_and(|end| end > facts.pulled_source_end)
            {
                return Err(Error::InvalidReport);
            }
        }
        Ok(Self {
            epoch,
            basis,
            identity,
            query,
            facts,
            native_evidence,
        })
    }
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
    pub const fn basis(&self) -> OutputFrameBasis {
        self.basis
    }
    pub const fn identity(&self) -> SourcePlaybackIdentity {
        self.identity
    }
    pub const fn query(&self) -> beatkernel::audio::InterpolatedTargetTime {
        self.query
    }
    pub const fn facts(&self) -> SourcePlaybackFacts {
        self.facts
    }
    pub const fn native_evidence(&self) -> Option<&E> {
        self.native_evidence.as_ref()
    }
}

/// A native coordinate and the exact source query from its coherent publication.
#[derive(Clone, Copy, Debug)]
pub struct NativePlaybackObservation<E> {
    coordinate: crate::native_output_coordinate::CoordinateNativeAudioSnapshot,
    source: NativeSourcePlaybackQuery<E>,
}
impl<E> NativePlaybackObservation<E> {
    pub fn new(
        coordinate: crate::native_output_coordinate::CoordinateNativeAudioSnapshot,
        source: NativeSourcePlaybackQuery<E>,
    ) -> Result<Self, SourcePlaybackPacketError> {
        use crate::native_output_coordinate::NativeOutputCoordinate;
        use SourcePlaybackPacketError as Error;
        if source.native_evidence.is_none() {
            return Err(Error::MissingNativeEvidence);
        }
        let snapshot = coordinate.snapshot();
        if snapshot.epoch != source.epoch {
            return Err(Error::EpochMismatch);
        }
        if snapshot.basis != source.basis {
            return Err(Error::BasisMismatch);
        }
        let order = match coordinate.coordinate() {
            NativeOutputCoordinate::StreamFrame { frame } => {
                let physical = source
                    .basis
                    .start_physical_frame()
                    .checked_add(frame)
                    .ok_or(Error::Overflow)?;
                source
                    .query
                    .compare_source_time_frame(physical, source.identity.hz)
                    .map_err(|_| Error::Overflow)?
            }
            NativeOutputCoordinate::Counter {
                position,
                frequency,
            } => {
                let base = beatkernel::audio::TargetTime::from_frames(
                    source.basis.start_physical_frame(),
                    source.identity.hz,
                )
                .map_err(|_| Error::Overflow)?;
                source
                    .query
                    .compare_native_time_with_offset(base, position, frequency, 0)
                    .map_err(|_| Error::Overflow)?
            }
        };
        if !order.is_eq() {
            return Err(Error::CoordinateMismatch);
        }
        Ok(Self { coordinate, source })
    }
    pub const fn coordinate(
        &self,
    ) -> crate::native_output_coordinate::CoordinateNativeAudioSnapshot {
        self.coordinate
    }
    pub const fn source(&self) -> &NativeSourcePlaybackQuery<E> {
        &self.source
    }
    pub fn into_source(self) -> NativeSourcePlaybackQuery<E> {
        self.source
    }
}

/// Opt-in source-grid facts beside original native coordinates. No new clock authority.
pub trait SourcePlaybackNativeOutputBackend<O: SoftwareOutputState = Mixer>:
    OriginalCoordinateNativeOutputBackend<O>
{
    type NativeEvidence;
    fn observe_native_playback(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Self::NativeEvidence>>, Self::Error>;
    fn source_playback_at(
        &self,
        output: &Self::Output,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Self::NativeEvidence>>, Self::Error>;
}

/// Source and Target share decision states, not pending-tail policy.
pub type SourceRetirementStatus = TargetRetirementStatus;

/// Conditional permission from the sole Source worker, retaining the complete
/// software owner through the inherited generic lifecycle and recovery contract.
pub trait ConditionalSourceRetirementNativeOutputBackend<O: SoftwareOutputState = Mixer>:
    SourcePlaybackNativeOutputBackend<O>
{
    /// Binds the original control allocation and a checked request nonce.
    type Ticket;
    /// Only raw native IO or liveness failures may be deferred after confirmed
    /// retirement authorization. Typed packet, identity and timing rejections
    /// remain visible even when an adapter returns them in its backend error.
    /// Unclassified errors are preserved by default.
    fn is_source_native_io_error(&self, _error: &Self::Error) -> bool {
        false
    }
    /// Actual immutable descriptor of the newly created native output. Only
    /// SourceFrame or SourceCounter is valid; Counter keeps its actual native
    /// frequency. Created metadata and prefill grant no native progress.
    fn created_source_coordinate_basis(
        &self,
        output: &Self::Output,
    ) -> Result<crate::audio_authority::AudioAuthorityCoordinateBasis, Self::Error>;
    /// Cold validation includes request, capacity and the exact requested matrix
    /// before asking the worker to inspect its actual pending PCM. None means
    /// busy or an unconsumed outcome; neither may overwrite the existing ticket.
    fn submit_source_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        request: &Self::Request,
        matrix: Option<&beatkernel::audio::ChannelMatrix>,
    ) -> Result<Option<Self::Ticket>, Self::Error>;
    /// Authorization is sticky permission, not proof of Stop, join or recovery.
    fn poll_source_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<SourceRetirementStatus, Self::Error>;
    /// Cancellation and authorization share one decision; an already authorized
    /// output cannot be restored by cancelling its ticket.
    fn cancel_source_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<SourceRetirementStatus, Self::Error>;
}

/// Queues an original Source-history minimum on the statically owned output.
/// A revision acknowledges the request only, never native playback or retirement.
pub trait SourceHistoryRetirementNativeOutputBackend<O: SoftwareOutputState = Mixer>:
    SourcePlaybackNativeOutputBackend<O>
{
    /// Rechecks the actual output epoch, basis and original history identity
    /// before effect. Busy returns `None`; the caller includes every live pin.
    fn request_source_history_retirement(
        &mut self,
        output: &mut Self::Output,
        expected_epoch: u64,
        expected_basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        source_pin: u64,
    ) -> Result<Option<u64>, Self::Error>;
}

/// Lifecycle ownership does not imply a fixed source-grid timing capability.
/// Converted backends use `TargetFrameBasis` and retain their complete owner.
pub trait OutputReplacementBackend<O = Mixer, Basis = OutputFrameBasis> {
    type Presentation: GameplayPresentationPort;
    type Output: StoppedMixerSource<O, Error = Self::Error>;
    type Request;
    type Error;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: O,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output, O>>;
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error>;
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error>;
    /// Cold setup immediately before a paused replacement starts. Initial
    /// stream startup does not invoke this hook; source-grid backends need no setup.
    fn prepare_replacement_start(&mut self, _: &mut Self::Output) -> Result<(), Self::Error> {
        Ok(())
    }
    fn epoch(&self, output: &Self::Output) -> u64;
    fn basis(&self, output: &Self::Output) -> Basis;
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error>;
    /// Adapter may defer native pairs preceding the retained-tail pause frontier.
    fn observe_replacement(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
        _: &crate::playback_pause::NativePause,
    ) -> Result<(), Self::Error> {
        self.observe(output, presentation)
    }
    /// Interval backends override this with their original accepted end evidence.
    fn observe_end(
        &self,
        _: &Self::Output,
        end: &mut crate::native_end::NativeEnd,
        pair: ClockPair,
        report: Option<RenderReport>,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        Ok(end.observe(report, pair)?)
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error>;
    fn pause_observation(
        &self,
        _: &Self::Output,
        pair: ClockPair,
        _: ClockPoint,
    ) -> Result<LivePauseObservation, Self::Error> {
        Ok(LivePauseObservation::Point(pair))
    }
}

/// Original native evidence without invoking the optional legacy estimator.
pub trait OriginalNativeOutputBackend<O: SoftwareOutputState = Mixer>:
    OutputReplacementBackend<O>
{
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error>;
}

/// Original observations for a target-time output, independent of source pull.
/// Native lifecycle is shared; this port adds no alternative clock discipline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetOutputTelemetry {
    pub source: Option<RenderReport>,
    pub converted: Option<beatkernel::audio::ConvertedRenderReport>,
    pub facts: beatkernel::audio::ConvertedBoundaryFacts,
}

pub trait OriginalTargetNativeOutputBackend<O>:
    OutputReplacementBackend<O, beatkernel::audio::TargetFrameBasis>
{
    /// Validate pending interpretation and plan the new immutable creation basis
    /// before transferring the recovered owner to the native worker.
    fn planned_target_basis(
        &self,
        request: &Self::Request,
        owner: &O,
    ) -> Result<beatkernel::audio::TargetFrameBasis, Self::Error>;
    fn observe_native_target(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::TargetNativeAudioSnapshot>, Self::Error>;
    fn converted_report(
        &self,
        output: &Self::Output,
    ) -> Result<Option<beatkernel::audio::ConvertedRenderReport>, Self::Error>;
    fn boundary_facts(
        &self,
        output: &Self::Output,
    ) -> Result<beatkernel::audio::ConvertedBoundaryFacts, Self::Error>;
    /// The default is suitable only for serialized same-owner implementations.
    /// Concurrent worker adapters must override with one coherent bounded read.
    fn output_telemetry(
        &self,
        output: &Self::Output,
    ) -> Result<Option<TargetOutputTelemetry>, Self::Error> {
        let source = self.render_report(output)?;
        let converted = self.converted_report(output)?;
        let facts = self.boundary_facts(output)?;
        Ok(
            (facts != beatkernel::audio::ConvertedBoundaryFacts::default()).then_some(
                TargetOutputTelemetry {
                    source,
                    converted,
                    facts,
                },
            ),
        )
    }
    /// Paused replacements select held output before native `start`; ordinary
    /// transitions still wait for actual source adoption and native crossing.
    fn set_held(&mut self, output: &mut Self::Output, held: bool) -> Result<(), Self::Error>;
}

/// Explicit same-rate channel conversion; no default can silently ignore a matrix.
pub trait OutputChannelRemixBackend<O: SoftwareOutputState = Mixer>:
    OutputReplacementBackend<O>
{
    fn open_remixed(
        &mut self,
        request: Self::Request,
        mixer: O,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output, O>>;
}

pub trait OutputUiPort {
    fn advertise(&mut self, capability: Option<OutputCapability>) -> io::Result<()>;
    fn take_request(&mut self) -> io::Result<Option<OutputRequest>>;
    fn reply(&mut self, reply: &OutputReply) -> io::Result<()>;
    fn pending(&self) -> bool;
}

/// Opt-in original coordinates; implementations must retain their actual observation.
pub trait OriginalCoordinateNativeOutputBackend<O: SoftwareOutputState = Mixer>:
    OriginalNativeOutputBackend<O>
{
    fn observe_native_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_output_coordinate::CoordinateNativeAudioSnapshot>, Self::Error>;
}

/// Target native units, independent of the source converter's pull frontier.
pub trait OriginalCoordinateTargetNativeOutputBackend<O>:
    OriginalTargetNativeOutputBackend<O>
{
    fn observe_native_target_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<
        Option<crate::native_output_coordinate::CoordinateTargetNativeAudioSnapshot>,
        Self::Error,
    >;
}
