//! Trusted same-thread ASIO acquisition and original interval evidence.
use crate::{
    gameplay::output::adapters::observation::{
        checked_asio_pause_observation, observe_asio_with_basis, ReplacementObservationError,
    },
    gameplay::output::ports::OutputReplacementBackend,
    live_pause::LivePauseObservation,
};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    time::{ClockPair, ClockPoint},
};
use beatkernel_platform::{
    audio::{
        asio::{
            AsioBufferRequest, AsioPresentationError, AsioPresentationObservation,
            MultimediaClockAnchor,
        },
        presentation::discipline::PresentationDiscipline,
    },
    windows::{
        asio::{
            control::TrustedAsioOpener,
            stream::{AsioPrepareFailure, AsioStream, AsioStreamError, AsioStreamPhase},
        },
        clock::QpcClock,
    },
};
#[derive(Debug)]
pub enum AsioReplacementError {
    Coordinate(crate::native_output_coordinate::NativeOutputCoordinateError),
    Native(AsioStreamError),
    Playback(crate::gameplay::output::ports::SourcePlaybackPacketError),
    Host(std::io::Error),
    Presentation(AsioPresentationError),
    Observation(ReplacementObservationError),
    Status(AsioStreamPhase),
}
impl std::fmt::Display for AsioReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Coordinate(error) => error.fmt(f),
            Self::Native(e) => e.fmt(f),
            Self::Playback(e) => e.fmt(f),
            Self::Host(e) => e.fmt(f),
            Self::Presentation(e) => e.fmt(f),
            Self::Observation(e) => e.fmt(f),
            Self::Status(phase) => write!(f, "ASIO replacement output phase: {phase:?}"),
        }
    }
}
impl std::error::Error for AsioReplacementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Coordinate(error) => Some(error),
            Self::Native(e) => Some(e),
            Self::Playback(e) => Some(e),
            Self::Host(e) => Some(e),
            Self::Presentation(e) => Some(e),
            Self::Observation(e) => Some(e),
            Self::Status(_) => None,
        }
    }
}
/// Sealed native permission plus exact channel/buffer settings; no driver is opened here.
pub struct AsioReplacementRequest {
    opener: TrustedAsioOpener,
    channels: Vec<u32>,
    buffer: AsioBufferRequest,
}
impl AsioReplacementRequest {
    pub fn new(opener: TrustedAsioOpener, channels: Vec<u32>, buffer: AsioBufferRequest) -> Self {
        Self {
            opener,
            channels,
            buffer,
        }
    }
}
pub struct AsioReplacementOutput {
    stream: AsioStream,
    epoch: u64,
    observation: Option<AsioPresentationObservation>,
}
impl AsioReplacementOutput {
    pub fn from_stream(stream: AsioStream) -> Self {
        Self {
            stream,
            epoch: 0,
            observation: None,
        }
    }
    pub fn stream(&self) -> &AsioStream {
        &self.stream
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
impl StoppedMixerSource for AsioReplacementOutput {
    type Error = AsioReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        self.stream
            .take_stopped_mixer()
            .map_err(AsioReplacementError::Native)
    }
}
fn map_driver_open_error(
    error: beatkernel_platform::windows::asio::control::AsioControlError,
    mixer: Mixer,
) -> OutputOpenFailure<AsioReplacementError, AsioReplacementOutput> {
    OutputOpenFailure::recovered(
        AsioReplacementError::Native(AsioStreamError::Control(error)),
        Some(mixer),
    )
}
fn map_prepare_failure(
    failure: AsioPrepareFailure,
    epoch: u64,
) -> OutputOpenFailure<AsioReplacementError, AsioReplacementOutput> {
    let (error, mixer, pending, cleanup) = failure.into_parts();
    let mapped = match pending {
        Some(stream) => OutputOpenFailure::pending(
            AsioReplacementError::Native(error),
            AsioReplacementOutput {
                stream,
                epoch,
                observation: None,
            },
        ),
        None => OutputOpenFailure::recovered(AsioReplacementError::Native(error), mixer),
    };
    match cleanup {
        Some(error) => mapped.with_cleanup_error(AsioReplacementError::Native(error)),
        None => mapped,
    }
}
pub struct AsioReplacementBackend {
    clock: QpcClock,
    anchor: MultimediaClockAnchor,
    latency_error_ns: u64,
}
impl AsioReplacementBackend {
    pub fn new(clock: QpcClock, anchor: MultimediaClockAnchor, latency_error_ns: u64) -> Self {
        Self {
            clock,
            anchor,
            latency_error_ns,
        }
    }
}
impl OutputReplacementBackend for AsioReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = AsioReplacementOutput;
    type Request = AsioReplacementRequest;
    type Error = AsioReplacementError;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        let control = match request.opener.open() {
            Ok(control) => control,
            Err(error) => return Err(map_driver_open_error(error, mixer)),
        };
        AsioStream::prepare_with_clock_recoverable(
            control,
            mixer,
            request.channels,
            request.buffer,
            self.clock,
        )
        .map(|stream| AsioReplacementOutput {
            stream,
            epoch,
            observation: None,
        })
        .map_err(|failure| map_prepare_failure(failure, epoch))
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.stop().map_err(AsioReplacementError::Native)
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.start().map_err(AsioReplacementError::Native)
    }
    fn epoch(&self, output: &Self::Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Self::Output) -> OutputFrameBasis {
        output.stream.frame_basis()
    }
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        observe_asio_with_basis(
            presentation,
            output.epoch,
            None,
            output.stream.frame_basis(),
        )
        .map_err(AsioReplacementError::Observation)?;
        let Some(observation) = self.original_observation(output)? else {
            return Ok(());
        };
        if observe_asio_with_basis(
            presentation,
            output.epoch,
            Some(observation),
            output.stream.frame_basis(),
        )
        .map_err(AsioReplacementError::Observation)?
        {
            output.observation = Some(observation);
        }
        Ok(())
    }
    fn observe_end(
        &self,
        output: &Self::Output,
        end: &mut crate::native_end::NativeEnd,
        _: ClockPair,
        _: Option<RenderReport>,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        match output.observation {
            Some(observation) => Ok(end.observe_asio(observation)?),
            None => Ok(None),
        }
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.observation.map(|observation| observation.render))
    }
    fn pause_observation(
        &self,
        output: &Self::Output,
        _: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, Self::Error> {
        checked_asio_pause_observation(output.observation, now)
            .map_err(AsioReplacementError::Presentation)
    }
}

impl crate::gameplay::output::ports::OutputChannelRemixBackend for AsioReplacementBackend {
    fn open_remixed(
        &mut self,
        request: AsioReplacementRequest,
        mixer: Mixer,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<AsioReplacementOutput, OutputOpenFailure<AsioReplacementError, AsioReplacementOutput>>
    {
        let control = match request.opener.open() {
            Ok(control) => control,
            Err(error) => return Err(map_driver_open_error(error, mixer)),
        };
        AsioStream::prepare_remixed_recoverable(
            control,
            mixer,
            request.channels,
            request.buffer,
            matrix,
            Some(self.clock),
        )
        .map(|stream| AsioReplacementOutput {
            stream,
            epoch,
            observation: None,
        })
        .map_err(|failure| map_prepare_failure(failure, epoch))
    }
}
#[cfg(test)]
#[path = "asio_fixtures.rs"]
mod fixtures;

impl AsioReplacementBackend {
    fn original_observation(
        &mut self,
        output: &mut AsioReplacementOutput,
    ) -> Result<Option<AsioPresentationObservation>, AsioReplacementError> {
        let snapshot = output
            .stream
            .snapshot()
            .map_err(AsioReplacementError::Native)?;
        if snapshot.native.faults.0 != 0 || snapshot.native.render_error != 0 {
            return Err(AsioReplacementError::Native(
                AsioStreamError::ReopenRequired(snapshot.native),
            ));
        }
        match snapshot.phase {
            AsioStreamPhase::Ready => return Ok(None),
            AsioStreamPhase::Running => {}
            phase => return Err(AsioReplacementError::Status(phase)),
        }
        let now = self
            .clock
            .sample()
            .map_err(AsioReplacementError::Host)?
            .normalized;
        if self.anchor.refresh_due(now).map_err(|error| {
            AsioReplacementError::Presentation(AsioPresentationError::Clock(error))
        })? {
            let receipt = self
                .clock
                .sample_multimedia()
                .map_err(AsioReplacementError::Host)?;
            self.anchor = self
                .anchor
                .refreshed(
                    receipt.milliseconds,
                    receipt.before.normalized,
                    receipt.after.normalized,
                )
                .map_err(|error| {
                    AsioReplacementError::Presentation(AsioPresentationError::Clock(error))
                })?;
        }
        let output_origin = output.stream.frame_basis().origin();
        let observation = match output.stream.presentation_observation(
            &self.anchor,
            &self.clock,
            self.latency_error_ns,
            output_origin,
        ) {
            Ok(observation) => observation,
            Err(AsioPresentationError::Unavailable)
                if !snapshot.telemetry_available || snapshot.buffer_observation.is_none() =>
            {
                return Ok(None);
            }
            Err(error) => return Err(AsioReplacementError::Presentation(error)),
        };
        Ok(Some(observation))
    }
}
impl crate::gameplay::output::ports::OriginalNativeOutputBackend for AsioReplacementBackend {
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error> {
        Ok(self.original_observation(output)?.map(|observation| crate::native_audio_presentation::NativeAudioSnapshot {
            epoch: output.epoch, basis: output.stream.frame_basis(),
            evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Asio {
                observation, basis: Some(output.stream.frame_basis()),
            },
        }))
    }
}

impl crate::gameplay::output::ports::OriginalCoordinateNativeOutputBackend
    for AsioReplacementBackend
{
    fn observe_native_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_output_coordinate::CoordinateNativeAudioSnapshot>, Self::Error>
    {
        use crate::gameplay::output::ports::OriginalNativeOutputBackend;
        use crate::native_output_coordinate::{
            CoordinateNativeAudioSnapshot, NativeOutputCoordinate, NativeOutputCoordinateError,
        };
        self.observe_native(output)?.map(|snapshot| {
            let beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Asio { observation, .. } = snapshot.evidence
            else { return Err(AsioReplacementError::Coordinate(NativeOutputCoordinateError::UnsupportedEvidence)); };
            let frame = observation.render.start_frame.checked_sub(snapshot.basis.start_physical_frame())
                .ok_or(AsioReplacementError::Coordinate(NativeOutputCoordinateError::FrameBeforeBasis))?;
            CoordinateNativeAudioSnapshot::new(snapshot, NativeOutputCoordinate::StreamFrame { frame })
                .map_err(AsioReplacementError::Coordinate)
        }).transpose()
    }
}

use crate::gameplay::output::ports::{
    NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
    SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend, SourcePlaybackPacketError,
};
use crate::native_output_coordinate::CoordinateNativeAudioSnapshot;
use beatkernel::audio::InterpolatedTargetTime;
use beatkernel_platform::windows::asio::stream::{AsioBufferObservation, AsioPlaybackPhaseSnapshot};

/// Software planar-write bounds, with the original coherent callback retained.
/// Cached diagnostics in this view grant no fresh native presentation permission.
pub(crate) fn qualify_asio_source_packet(
    epoch: u64,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    view: AsioPlaybackPhaseSnapshot,
) -> Result<NativeSourcePlaybackQuery<AsioPlaybackPhaseSnapshot>, SourcePlaybackPacketError> {
    if view.revision == 0 || view.revision & 1 != 0 {
        return Err(SourcePlaybackPacketError::InvalidRevision);
    }
    if !view.snapshot.telemetry_available {
        return Err(SourcePlaybackPacketError::InvalidPrefix);
    }
    if view
        .snapshot
        .buffer_observation
        .is_some_and(|callback| view.snapshot.render != Some(callback.render))
    {
        return Err(SourcePlaybackPacketError::InvalidReport);
    }
    let prepared_end = basis
        .start_physical_frame()
        .checked_add(view.snapshot.prepared_frames)
        .ok_or(SourcePlaybackPacketError::Overflow)?;
    NativeSourcePlaybackQuery::new(
        epoch,
        basis,
        identity,
        query,
        SourcePlaybackFacts {
            playback: view.playback,
            pulled_source_end: view.pulled_source_end,
            submitted_source_end: prepared_end,
            render: view.snapshot.render,
            revision: view.revision,
        },
        Some(view),
    )
}

/// A changed successful callback publication waits for another bounded read.
/// Neither generated source history nor cached diagnostics authenticate HOST.
/// This performs only mathematical qualification of the supplied pair. The caller
/// must first acquire a genuine original SDK/clock observation through its native
/// observation path; this function grants no native presentation permission and
/// does not change clock anchors.
pub fn qualify_asio_source_observation_packet(
    candidate: CoordinateNativeAudioSnapshot,
    expected_callback: AsioBufferObservation,
    packet: NativeSourcePlaybackQuery<AsioPlaybackPhaseSnapshot>,
) -> Result<Option<NativePlaybackObservation<AsioPlaybackPhaseSnapshot>>, SourcePlaybackPacketError>
{
    let view = packet
        .native_evidence()
        .ok_or(SourcePlaybackPacketError::MissingNativeEvidence)?;
    let beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Asio {
        observation, basis,
    } = candidate.snapshot().evidence else {
        return Err(SourcePlaybackPacketError::CoordinateMismatch);
    };
    if candidate.snapshot().epoch != packet.epoch() {
        return Err(SourcePlaybackPacketError::EpochMismatch);
    }
    if candidate.snapshot().basis != packet.basis()
        || basis != Some(packet.basis())
        || observation.sample_rate != packet.basis().sample_rate()
        || observation.output_origin != packet.basis().origin()
    {
        return Err(SourcePlaybackPacketError::BasisMismatch);
    }
    if view.snapshot.buffer_observation != Some(expected_callback)
        || view.snapshot.render != Some(observation.render)
        || expected_callback.render != observation.render
    {
        return Ok(None);
    }
    NativePlaybackObservation::new(candidate, packet).map(Some)
}

impl AsioReplacementBackend {
    /// Reads the existing coherent Source publication from a borrowed stream.
    /// This immutable route makes no SDK calls and grants no native presentation
    /// permission; its diagnostics are the stream's cached diagnostics.
    pub fn source_playback_for_stream(
        stream: &AsioStream,
        epoch: u64,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<AsioPlaybackPhaseSnapshot>>, AsioReplacementError>
    {
        let basis = stream.frame_basis();
        if basis.sample_rate() != identity.hz()
            || identity.initial_physical() > basis.start_physical_frame()
        {
            return Err(AsioReplacementError::Playback(
                SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        stream
            .source_playback_view_at(
                query,
                identity.hz(),
                identity.initial_physical(),
                identity.initial_playback(),
            )
            .map(|view| {
                qualify_asio_source_packet(epoch, basis, identity, *query, view)
                    .map_err(AsioReplacementError::Playback)
            })
            .transpose()
    }
}

impl SourcePlaybackNativeOutputBackend for AsioReplacementBackend {
    type NativeEvidence = AsioPlaybackPhaseSnapshot;
    fn source_playback_at(
        &self,
        output: &Self::Output,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Self::NativeEvidence>>, Self::Error> {
        Self::source_playback_for_stream(&output.stream, output.epoch, query, identity)
    }
    fn observe_native_playback(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Self::NativeEvidence>>, Self::Error> {
        use crate::gameplay::output::ports::OriginalCoordinateNativeOutputBackend;
        let basis = output.stream.frame_basis();
        if basis.sample_rate() != identity.hz()
            || identity.initial_physical() > basis.start_physical_frame()
        {
            return Err(AsioReplacementError::Playback(
                SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        for _ in 0..3 {
            // Fresh SDK diagnostics and the copied original callback are acquired
            // through the existing owning-thread native path, never this view.
            let before = output
                .stream
                .snapshot()
                .map_err(AsioReplacementError::Native)?;
            if before.native.faults.0 != 0 || before.native.render_error != 0 {
                return Err(AsioReplacementError::Native(
                    AsioStreamError::ReopenRequired(before.native),
                ));
            }
            match before.phase {
                AsioStreamPhase::Ready => return Ok(None),
                AsioStreamPhase::Running => {}
                phase => return Err(AsioReplacementError::Status(phase)),
            }
            let Some(callback) = before.buffer_observation else {
                continue;
            };
            let Some(candidate) = self.observe_native_coordinate(output)? else {
                continue;
            };
            let query = candidate
                .source_query()
                .map_err(AsioReplacementError::Coordinate)?;
            let Some(packet) = self.source_playback_at(output, &query, identity)? else {
                continue;
            };
            if let Some(observation) =
                qualify_asio_source_observation_packet(candidate, callback, packet)
                    .map_err(AsioReplacementError::Playback)?
            {
                return Ok(Some(observation));
            }
        }
        Ok(None)
    }
}

impl crate::gameplay::output::ports::SourceHistoryRetirementNativeOutputBackend
    for AsioReplacementBackend
{
    fn request_source_history_retirement(
        &mut self,
        output: &mut Self::Output,
        expected_epoch: u64,
        expected_basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        source_pin: u64,
    ) -> Result<Option<u64>, Self::Error> {
        if output.epoch != expected_epoch {
            return Err(AsioReplacementError::Playback(
                SourcePlaybackPacketError::EpochMismatch,
            ));
        }
        if output.stream.frame_basis() != expected_basis {
            return Err(AsioReplacementError::Playback(
                SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        output
            .stream
            .request_source_history_retirement(
                expected_basis,
                (
                    identity.hz(),
                    identity.initial_physical(),
                    identity.initial_playback(),
                ),
                source_pin,
            )
            .map_err(AsioReplacementError::Native)
    }
}

#[cfg(test)]
#[path = "asio_source_packet_fixtures.rs"]
mod source_packet_fixtures;
