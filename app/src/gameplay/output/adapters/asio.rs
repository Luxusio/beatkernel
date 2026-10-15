//! Trusted same-thread ASIO acquisition and original interval evidence.
use crate::{
    gameplay::output::adapters::observation::{
        checked_asio_pause_observation, observe_asio_with_basis, ReplacementObservationError,
    },
    gameplay::output::application::replacement::coordinate_source::source_frame_observations::PendingSourceFrameObservations,
    gameplay::output::ports::OutputReplacementBackend,
    live_pause::LivePauseObservation,
};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    time::{ClockDomainId, ClockPair, ClockPoint},
};
use beatkernel_platform::{
    audio::{
        asio::{
            AsioBufferRequest, AsioPresentationError, AsioPresentationObservation,
            MultimediaClockAnchor,
        },
        presentation::discipline::PresentationDiscipline,
        NativeOutputState,
    },
    windows::{
        asio::{
            control::TrustedAsioOpener,
            stream::{
                AsioPrepareFailure, AsioStatePrepareFailure, AsioStream, AsioStreamError,
                AsioStreamPhase,
            },
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
    observation_bounds: Option<(MultimediaClockAnchor, u64)>,
}
impl AsioReplacementRequest {
    pub fn new(opener: TrustedAsioOpener, channels: Vec<u32>, buffer: AsioBufferRequest) -> Self {
        Self {
            opener,
            channels,
            buffer,
            observation_bounds: None,
        }
    }
    /// Freezes the new complete output's native timer relation and latency bound.
    /// Existing outputs keep their own relation when this request is applied.
    pub fn with_observation_bounds(
        mut self,
        anchor: MultimediaClockAnchor,
        latency_error_ns: u64,
    ) -> Self {
        self.observation_bounds = Some((anchor, latency_error_ns));
        self
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
        original_stream_observation(
            &mut output.stream,
            self.clock,
            &mut self.anchor,
            self.latency_error_ns,
        )
    }
}

fn checked_stream_snapshot(
    stream: &mut AsioStream,
) -> Result<beatkernel_platform::windows::asio::stream::AsioStreamSnapshot, AsioReplacementError> {
    let snapshot = stream.snapshot().map_err(AsioReplacementError::Native)?;
    if snapshot.native.faults.0 != 0 || snapshot.native.render_error != 0 {
        return Err(AsioReplacementError::Native(
            AsioStreamError::ReopenRequired(snapshot.native),
        ));
    }
    match snapshot.phase {
        AsioStreamPhase::Ready | AsioStreamPhase::Running => Ok(snapshot),
        phase => Err(AsioReplacementError::Status(phase)),
    }
}

fn original_stream_observation(
    stream: &mut AsioStream,
    clock: QpcClock,
    anchor: &mut MultimediaClockAnchor,
    latency_error_ns: u64,
) -> Result<Option<AsioPresentationObservation>, AsioReplacementError> {
    let snapshot = checked_stream_snapshot(stream)?;
    if snapshot.phase == AsioStreamPhase::Ready {
        return Ok(None);
    }
    let now = clock
        .sample()
        .map_err(AsioReplacementError::Host)?
        .normalized;
    if anchor
        .refresh_due(now)
        .map_err(|error| AsioReplacementError::Presentation(AsioPresentationError::Clock(error)))?
    {
        let receipt = clock
            .sample_multimedia()
            .map_err(AsioReplacementError::Host)?;
        *anchor = anchor
            .refreshed(
                receipt.milliseconds,
                receipt.before.normalized,
                receipt.after.normalized,
            )
            .map_err(|error| {
                AsioReplacementError::Presentation(AsioPresentationError::Clock(error))
            })?;
    }
    let output_origin = stream.frame_basis().origin();
    match stream.presentation_observation(anchor, &clock, latency_error_ns, output_origin) {
        Ok(observation) => Ok(Some(observation)),
        Err(AsioPresentationError::Unavailable)
            if !snapshot.telemetry_available || snapshot.buffer_observation.is_none() =>
        {
            Ok(None)
        }
        Err(error) => Err(AsioReplacementError::Presentation(error)),
    }
}

fn original_native_snapshot(
    epoch: u64,
    basis: OutputFrameBasis,
    observation: AsioPresentationObservation,
) -> crate::native_audio_presentation::NativeAudioSnapshot {
    crate::native_audio_presentation::NativeAudioSnapshot {
        epoch,
        basis,
        evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Asio {
            observation,
            basis: Some(basis),
        },
    }
}

fn original_native_coordinate(
    snapshot: crate::native_audio_presentation::NativeAudioSnapshot,
) -> Result<CoordinateNativeAudioSnapshot, AsioReplacementError> {
    use crate::native_output_coordinate::{NativeOutputCoordinate, NativeOutputCoordinateError};
    let beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Asio {
        observation, ..
    } = snapshot.evidence else {
        return Err(AsioReplacementError::Coordinate(NativeOutputCoordinateError::UnsupportedEvidence));
    };
    let frame = observation
        .delivered_first_frame
        .checked_sub(snapshot.basis.start_physical_frame())
        .ok_or(AsioReplacementError::Coordinate(
            NativeOutputCoordinateError::FrameBeforeBasis,
        ))?;
    CoordinateNativeAudioSnapshot::new(snapshot, NativeOutputCoordinate::StreamFrame { frame })
        .map_err(AsioReplacementError::Coordinate)
}

impl crate::gameplay::output::ports::OriginalNativeOutputBackend for AsioReplacementBackend {
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error> {
        Ok(self.original_observation(output)?.map(|observation| {
            original_native_snapshot(output.epoch, output.stream.frame_basis(), observation)
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
        self.observe_native(output)?
            .map(original_native_coordinate)
            .transpose()
    }
}

use crate::gameplay::output::ports::{
    NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
    SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend, SourcePlaybackPacketError,
};
use crate::native_output_coordinate::CoordinateNativeAudioSnapshot;
use beatkernel::audio::InterpolatedTargetTime;
use beatkernel_platform::windows::asio::stream::{
    AsioBufferObservation, AsioPlaybackPhaseSnapshot,
};

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
    if let Some(callback) = view.snapshot.buffer_observation {
        let delivered_end = callback
            .delivered_first_frame
            .checked_add(u64::from(callback.delivered_frames))
            .ok_or(SourcePlaybackPacketError::Overflow)?;
        let report_end = callback
            .render
            .start_frame
            .checked_add(
                u64::try_from(callback.render.frames)
                    .map_err(|_| SourcePlaybackPacketError::Overflow)?,
            )
            .ok_or(SourcePlaybackPacketError::Overflow)?;
        if callback.delivered_frames == 0
            || callback.delivered_first_frame < callback.render.start_frame
            || delivered_end > report_end
            || delivered_end != prepared_end
        {
            return Err(SourcePlaybackPacketError::InvalidPrefix);
        }
    }
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
        || expected_callback.delivered_first_frame != observation.delivered_first_frame
        || usize::try_from(expected_callback.delivered_frames).ok()
            != Some(observation.delivered_frames)
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
        observe_stream_playback(
            &mut output.stream,
            output.epoch,
            self.clock,
            &mut self.anchor,
            self.latency_error_ns,
            identity,
        )
    }
}

fn observe_stream_playback(
    stream: &mut AsioStream,
    epoch: u64,
    clock: QpcClock,
    anchor: &mut MultimediaClockAnchor,
    latency_error_ns: u64,
    identity: SourcePlaybackIdentity,
) -> Result<Option<NativePlaybackObservation<AsioPlaybackPhaseSnapshot>>, AsioReplacementError> {
    let basis = stream.frame_basis();
    if basis.sample_rate() != identity.hz()
        || identity.initial_physical() > basis.start_physical_frame()
    {
        return Err(AsioReplacementError::Playback(
            SourcePlaybackPacketError::BasisMismatch,
        ));
    }
    for _ in 0..3 {
        let before = checked_stream_snapshot(stream)?;
        if before.phase == AsioStreamPhase::Ready {
            return Ok(None);
        }
        let Some(callback) = before.buffer_observation else {
            continue;
        };
        let Some(observation) =
            original_stream_observation(stream, clock, anchor, latency_error_ns)?
        else {
            continue;
        };
        let candidate =
            original_native_coordinate(original_native_snapshot(epoch, basis, observation))?;
        let query = candidate
            .source_query()
            .map_err(AsioReplacementError::Coordinate)?;
        let Some(packet) =
            AsioReplacementBackend::source_playback_for_stream(stream, epoch, &query, identity)?
        else {
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
        request_stream_source_history_retirement(
            &output.stream,
            output.epoch,
            expected_epoch,
            expected_basis,
            identity,
            source_pin,
        )
    }
}

#[cfg(test)]
#[path = "asio_source_packet_fixtures.rs"]
mod source_packet_fixtures;

fn request_stream_source_history_retirement(
    stream: &AsioStream,
    epoch: u64,
    expected_epoch: u64,
    expected_basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    source_pin: u64,
) -> Result<Option<u64>, AsioReplacementError> {
    if epoch != expected_epoch {
        return Err(AsioReplacementError::Playback(
            SourcePlaybackPacketError::EpochMismatch,
        ));
    }
    if stream.frame_basis() != expected_basis {
        return Err(AsioReplacementError::Playback(
            SourcePlaybackPacketError::BasisMismatch,
        ));
    }
    stream
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

/// Complete ownership and per-output timing configuration for retained Source IO.
pub struct AsioCompleteReplacementOutput {
    common: AsioReplacementOutput,
    anchor: MultimediaClockAnchor,
    latency_error_ns: u64,
    replacement_observations: Option<PendingSourceFrameObservations<AsioPlaybackPhaseSnapshot>>,
}
impl AsioCompleteReplacementOutput {
    pub fn from_stream(
        stream: AsioStream,
        anchor: MultimediaClockAnchor,
        latency_error_ns: u64,
    ) -> Self {
        Self {
            common: AsioReplacementOutput::from_stream(stream),
            anchor,
            latency_error_ns,
            replacement_observations: None,
        }
    }
    pub fn stream(&self) -> &AsioStream {
        &self.common.stream
    }
    pub fn stream_mut(&mut self) -> &mut AsioStream {
        &mut self.common.stream
    }
    pub const fn epoch(&self) -> u64 {
        self.common.epoch
    }
    pub fn channel_matrix(&self) -> Option<&beatkernel::audio::ChannelMatrix> {
        self.common.stream.source_channel_matrix()
    }
}
impl StoppedMixerSource<NativeOutputState> for AsioCompleteReplacementOutput {
    type Error = AsioReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<NativeOutputState>, Self::Error> {
        self.common
            .stream
            .take_stopped_output_state()
            .map_err(AsioReplacementError::Native)
    }
}

fn map_state_driver_open_error(
    error: beatkernel_platform::windows::asio::control::AsioControlError,
    state: NativeOutputState,
) -> OutputOpenFailure<AsioReplacementError, AsioCompleteReplacementOutput, NativeOutputState> {
    OutputOpenFailure::recovered_state(
        AsioReplacementError::Native(AsioStreamError::Control(error)),
        Some(state),
    )
}

fn map_state_prepare_failure(
    failure: AsioStatePrepareFailure,
    epoch: u64,
    anchor: MultimediaClockAnchor,
    latency_error_ns: u64,
) -> OutputOpenFailure<AsioReplacementError, AsioCompleteReplacementOutput, NativeOutputState> {
    let (error, state, pending, cleanup) = failure.into_parts();
    let mapped = match pending {
        Some(stream) => OutputOpenFailure::pending_state(
            AsioReplacementError::Native(error),
            AsioCompleteReplacementOutput {
                common: AsioReplacementOutput {
                    stream,
                    epoch,
                    observation: None,
                },
                anchor,
                latency_error_ns,
                replacement_observations: None,
            },
        ),
        None => OutputOpenFailure::recovered_state(AsioReplacementError::Native(error), state),
    };
    match cleanup {
        Some(error) => mapped.with_cleanup_error(AsioReplacementError::Native(error)),
        None => mapped,
    }
}

pub struct AsioCompleteReplacementBackend {
    clock: QpcClock,
    default_anchor: MultimediaClockAnchor,
    default_latency_error_ns: u64,
    host_domain: ClockDomainId,
    history_headroom: usize,
}
impl AsioCompleteReplacementBackend {
    pub fn new(
        clock: QpcClock,
        default_anchor: MultimediaClockAnchor,
        default_latency_error_ns: u64,
        host_domain: ClockDomainId,
        history_headroom: usize,
    ) -> Self {
        Self {
            clock,
            default_anchor,
            default_latency_error_ns,
            host_domain,
            history_headroom,
        }
    }
    fn open_complete(
        &mut self,
        request: AsioReplacementRequest,
        state: NativeOutputState,
        epoch: u64,
        matrix: Option<beatkernel::audio::ChannelMatrix>,
    ) -> Result<
        AsioCompleteReplacementOutput,
        OutputOpenFailure<AsioReplacementError, AsioCompleteReplacementOutput, NativeOutputState>,
    > {
        let (anchor, latency_error_ns) = request
            .observation_bounds
            .unwrap_or((self.default_anchor, self.default_latency_error_ns));
        if anchor.before().domain != self.host_domain {
            return Err(OutputOpenFailure::recovered_state(
                AsioReplacementError::Playback(SourcePlaybackPacketError::CoordinateMismatch),
                Some(state),
            ));
        }
        let control = match request.opener.open() {
            Ok(control) => control,
            Err(error) => return Err(map_state_driver_open_error(error, state)),
        };
        let prepared = match matrix {
            Some(matrix) => AsioStream::prepare_state_remixed_recoverable(
                control,
                state,
                request.channels,
                request.buffer,
                matrix,
                self.clock,
                self.history_headroom,
            ),
            None => AsioStream::prepare_state_recoverable(
                control,
                state,
                request.channels,
                request.buffer,
                self.clock,
                self.history_headroom,
            ),
        };
        prepared
            .map(|stream| AsioCompleteReplacementOutput {
                common: AsioReplacementOutput {
                    stream,
                    epoch,
                    observation: None,
                },
                anchor,
                latency_error_ns,
                replacement_observations: None,
            })
            .map_err(|failure| map_state_prepare_failure(failure, epoch, anchor, latency_error_ns))
    }
    fn original_observation(
        &self,
        output: &mut AsioCompleteReplacementOutput,
    ) -> Result<Option<AsioPresentationObservation>, AsioReplacementError> {
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        let observation = original_stream_observation(
            &mut output.common.stream,
            self.clock,
            &mut output.anchor,
            output.latency_error_ns,
        )?;
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        Ok(observation)
    }
}
impl OutputReplacementBackend<NativeOutputState> for AsioCompleteReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = AsioCompleteReplacementOutput;
    type Request = AsioReplacementRequest;
    type Error = AsioReplacementError;
    fn open(
        &mut self,
        request: Self::Request,
        state: NativeOutputState,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output, NativeOutputState>> {
        self.open_complete(request, state, epoch, None)
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .common
            .stream
            .stop()
            .map_err(AsioReplacementError::Native)
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .common
            .stream
            .start()
            .map_err(AsioReplacementError::Native)
    }
    fn epoch(&self, output: &Self::Output) -> u64 {
        output.common.epoch
    }
    fn basis(&self, output: &Self::Output) -> OutputFrameBasis {
        output.common.stream.frame_basis()
    }
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        let basis = output.common.stream.frame_basis();
        observe_asio_with_basis(presentation, output.common.epoch, None, basis)
            .map_err(AsioReplacementError::Observation)?;
        if let Some(observation) = self.original_observation(output)? {
            if observe_asio_with_basis(presentation, output.common.epoch, Some(observation), basis)
                .map_err(AsioReplacementError::Observation)?
            {
                output.common.observation = Some(observation);
            }
        }
        Ok(())
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        Ok(output
            .common
            .observation
            .map(|observation| observation.render))
    }
    fn observe_end(
        &self,
        output: &Self::Output,
        end: &mut crate::native_end::NativeEnd,
        _: ClockPair,
        _: Option<RenderReport>,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        match output.common.observation {
            Some(observation) => Ok(end.observe_asio(observation)?),
            None => Ok(None),
        }
    }
    fn pause_observation(
        &self,
        output: &Self::Output,
        _: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, Self::Error> {
        checked_asio_pause_observation(output.common.observation, now)
            .map_err(AsioReplacementError::Presentation)
    }
}
impl crate::gameplay::output::ports::OutputChannelRemixBackend<NativeOutputState>
    for AsioCompleteReplacementBackend
{
    fn open_remixed(
        &mut self,
        request: AsioReplacementRequest,
        state: NativeOutputState,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<
        AsioCompleteReplacementOutput,
        OutputOpenFailure<AsioReplacementError, AsioCompleteReplacementOutput, NativeOutputState>,
    > {
        self.open_complete(request, state, epoch, Some(matrix))
    }
}
impl crate::gameplay::output::ports::OriginalNativeOutputBackend<NativeOutputState>
    for AsioCompleteReplacementBackend
{
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error> {
        Ok(self.original_observation(output)?.map(|observation| {
            output.common.observation = Some(observation);
            original_native_snapshot(
                output.common.epoch,
                output.common.stream.frame_basis(),
                observation,
            )
        }))
    }
}
impl crate::gameplay::output::ports::OriginalCoordinateNativeOutputBackend<NativeOutputState>
    for AsioCompleteReplacementBackend
{
    fn observe_native_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<CoordinateNativeAudioSnapshot>, Self::Error> {
        use crate::gameplay::output::ports::OriginalNativeOutputBackend;
        self.observe_native(output)?
            .map(original_native_coordinate)
            .transpose()
    }
}
impl SourcePlaybackNativeOutputBackend<NativeOutputState> for AsioCompleteReplacementBackend {
    type NativeEvidence = AsioPlaybackPhaseSnapshot;
    fn source_playback_at(
        &self,
        output: &Self::Output,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Self::NativeEvidence>>, Self::Error> {
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        let packet = AsioReplacementBackend::source_playback_for_stream(
            &output.common.stream,
            output.common.epoch,
            query,
            identity,
        )?;
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        Ok(packet)
    }
    fn observe_native_playback(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Self::NativeEvidence>>, Self::Error> {
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        let observation = observe_stream_playback(
            &mut output.common.stream,
            output.common.epoch,
            self.clock,
            &mut output.anchor,
            output.latency_error_ns,
            identity,
        )?;
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        if let Some(observation) = &observation {
            if let beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::Asio { observation, .. }
                = observation.coordinate().snapshot().evidence {
                output.common.observation = Some(observation);
            }
        }
        Ok(observation)
    }
    fn observe_native_playback_replacement_at(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
        now: ClockPoint,
    ) -> Result<Option<NativePlaybackObservation<Self::NativeEvidence>>, Self::Error> {
        if now.domain != self.host_domain {
            return Err(AsioReplacementError::Playback(
                SourcePlaybackPacketError::CoordinateMismatch,
            ));
        }
        // Every poll acquires current diagnostics and Source liveness before a
        // retained original interval may be released, including a full cache.
        let observation = self.observe_native_playback(output, identity)?;
        let snapshot = checked_stream_snapshot(&mut output.common.stream)?;
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        if snapshot.phase == AsioStreamPhase::Ready {
            output.replacement_observations = None;
            return Ok(None);
        }
        let epoch = output.common.epoch;
        let basis = output.common.stream.frame_basis();
        if output.replacement_observations.is_none() {
            output.replacement_observations = Some(
                PendingSourceFrameObservations::new_asio(epoch, basis, identity, self.host_domain)
                    .map_err(AsioReplacementError::Playback)?,
            );
        }
        let pending = output
            .replacement_observations
            .as_mut()
            .expect("initialized above");
        pending
            .validate_expectations(epoch, basis, identity, self.host_domain)
            .map_err(AsioReplacementError::Playback)?;
        if let Some(observation) = observation {
            pending
                .push(observation)
                .map_err(AsioReplacementError::Playback)?;
        }
        checked_stream_snapshot(&mut output.common.stream)?;
        output
            .common
            .stream
            .source_liveness()
            .map_err(AsioReplacementError::Native)?;
        pending
            .take_covered(now)
            .map_err(AsioReplacementError::Playback)
    }
}
impl crate::gameplay::output::ports::SourceHistoryRetirementNativeOutputBackend<NativeOutputState>
    for AsioCompleteReplacementBackend
{
    fn request_source_history_retirement(
        &mut self,
        output: &mut Self::Output,
        expected_epoch: u64,
        expected_basis: OutputFrameBasis,
        identity: SourcePlaybackIdentity,
        source_pin: u64,
    ) -> Result<Option<u64>, Self::Error> {
        request_stream_source_history_retirement(
            &output.common.stream,
            output.common.epoch,
            expected_epoch,
            expected_basis,
            identity,
            source_pin,
        )
    }
}
impl
    crate::gameplay::output::ports::ConditionalSourceRetirementNativeOutputBackend<
        NativeOutputState,
    > for AsioCompleteReplacementBackend
{
    type Ticket = beatkernel_platform::windows::asio::stream::SourceRetirementTicket;
    fn is_source_native_io_error(&self, error: &Self::Error) -> bool {
        matches!(
            error,
            AsioReplacementError::Native(AsioStreamError::RecoveryUnavailable)
        )
    }
    fn created_source_coordinate_basis(
        &self,
        output: &Self::Output,
    ) -> Result<crate::audio_authority::AudioAuthorityCoordinateBasis, Self::Error> {
        Ok(
            crate::audio_authority::AudioAuthorityCoordinateBasis::SourceFrame(
                output.common.stream.frame_basis(),
            ),
        )
    }
    fn submit_source_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        request: &Self::Request,
        matrix: Option<&beatkernel::audio::ChannelMatrix>,
    ) -> Result<Option<Self::Ticket>, Self::Error> {
        output
            .common
            .stream
            .submit_source_conditional_retirement(&request.channels, request.buffer, matrix)
            .map_err(AsioReplacementError::Native)
    }
    fn poll_source_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<crate::gameplay::output::ports::SourceRetirementStatus, Self::Error> {
        output
            .common
            .stream
            .poll_source_conditional_retirement(ticket)
            .map(source_retirement_status)
            .map_err(AsioReplacementError::Native)
    }
    fn cancel_source_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<crate::gameplay::output::ports::SourceRetirementStatus, Self::Error> {
        output
            .common
            .stream
            .cancel_source_conditional_retirement(ticket)
            .map(source_retirement_status)
            .map_err(AsioReplacementError::Native)
    }
}
fn source_retirement_status(
    status: beatkernel_platform::windows::asio::stream::SourceRetirementStatus,
) -> crate::gameplay::output::ports::SourceRetirementStatus {
    use crate::gameplay::output::ports::SourceRetirementStatus as Status;
    use beatkernel_platform::windows::asio::stream::SourceRetirementStatus as Native;
    match status {
        Native::Pending => Status::Pending,
        Native::Refused => Status::Refused,
        Native::Cancelled => Status::Cancelled,
        Native::Authorized => Status::Authorized,
    }
}

#[cfg(test)]
#[path = "asio_complete_fixtures.rs"]
mod complete_fixtures;
