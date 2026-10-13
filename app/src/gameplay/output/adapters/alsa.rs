//! Actual ALSA lifecycle adapter for the portable paused-output replacement policy.
use crate::gameplay::output::ports::OutputReplacementBackend;
#[cfg(test)]
#[path = "alsa_target_fixtures.rs"]
mod target_fixtures;
#[cfg(test)]
#[path = "alsa_source_playback_fixtures.rs"]
mod source_playback_fixtures;
#[cfg(test)]
#[path = "alsa_target_playback_fixtures.rs"]
mod target_playback_fixtures;
use beatkernel::audio::{OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineError, PresentationDiscipline},
    audio::NativeOutputState,
    linux::{alsa_presentation_pair_with_basis, AlsaRequest, AlsaStatus, AlsaStream, LinuxError},
};

#[derive(Debug)]
pub enum AlsaReplacementError {
    Playback(crate::gameplay::output::ports::SourcePlaybackPacketError),
    TargetPlayback(crate::gameplay::output::ports::TargetPlaybackPacketError),
    Coordinate(crate::native_output_coordinate::NativeOutputCoordinateError),
    Linux(LinuxError),
    Discipline(DisciplineError),
    Status(AlsaStatus),
    EpochMismatch,
}
impl std::fmt::Display for AlsaReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Playback(error) => error.fmt(f),
            Self::TargetPlayback(error) => error.fmt(f),
            Self::Coordinate(error) => error.fmt(f),
            Self::Linux(error) => error.fmt(f),
            Self::Discipline(error) => error.fmt(f),
            Self::Status(status) => write!(f, "ALSA replacement output is not running: {status:?}"),
            Self::EpochMismatch => f.write_str("ALSA replacement observation epoch differs"),
        }
    }
}
impl std::error::Error for AlsaReplacementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Playback(error) => Some(error),
            Self::TargetPlayback(error) => Some(error),
            Self::Coordinate(error) => Some(error),
            Self::Linux(error) => Some(error),
            Self::Discipline(error) => Some(error),
            _ => None,
        }
    }
}
/// The creation token stays with the stream, including queued observations.
pub struct AlsaReplacementOutput {
    stream: AlsaStream,
    epoch: u64,
    matrix: Option<beatkernel::audio::ChannelMatrix>,
}
impl AlsaReplacementOutput {
    pub fn from_stream(stream: AlsaStream) -> Self {
        Self {
            stream,
            epoch: 0,
            matrix: None,
        }
    }
    pub fn stream(&self) -> &AlsaStream {
        &self.stream
    }
    pub fn stream_mut(&mut self) -> &mut AlsaStream {
        &mut self.stream
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn channel_matrix(&self) -> Option<&beatkernel::audio::ChannelMatrix> {
        self.matrix.as_ref()
    }
}
impl StoppedMixerSource<NativeOutputState> for AlsaReplacementOutput {
    type Error = AlsaReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<NativeOutputState>, Self::Error> {
        self.stream
            .take_stopped_output()
            .map_err(AlsaReplacementError::Linux)
    }
}
#[derive(Default)]
pub struct AlsaReplacementBackend;
impl OutputReplacementBackend<NativeOutputState> for AlsaReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = AlsaReplacementOutput;
    type Request = AlsaRequest;
    type Error = AlsaReplacementError;
    fn open(
        &mut self,
        request: AlsaRequest,
        mixer: NativeOutputState,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output, NativeOutputState>> {
        AlsaStream::open_state_recoverable(request, mixer)
            .map(|stream| AlsaReplacementOutput {
                stream,
                epoch,
                matrix: None,
            })
            .map_err(|failure| {
                let (error, mixer) = failure.into_parts();
                OutputOpenFailure::recovered_state(AlsaReplacementError::Linux(error), mixer)
            })
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.stop().map_err(AlsaReplacementError::Linux)
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.start().map_err(AlsaReplacementError::Linux)
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
        if presentation.epoch() != output.epoch {
            return Err(AlsaReplacementError::EpochMismatch);
        }
        if let Some(pair) = self.original_pair(output)? {
            presentation
                .observe_clock_pair_in_epoch(output.epoch, pair)
                .map_err(AlsaReplacementError::Discipline)?;
        }
        Ok(())
    }
    fn observe_replacement(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
        pause: &crate::playback_pause::NativePause,
    ) -> Result<(), Self::Error> {
        if presentation.epoch() != output.epoch {
            return Err(AlsaReplacementError::EpochMismatch);
        }
        if let Some(pair) = self.original_pair(output)? {
            if pause.replacement_observation_ready(output.stream.last_render_report(), pair) {
                presentation
                    .observe_clock_pair_in_epoch(output.epoch, pair)
                    .map_err(AlsaReplacementError::Discipline)?;
            }
        }
        Ok(())
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.stream.last_render_report())
    }
}

impl crate::gameplay::output::ports::OutputChannelRemixBackend<NativeOutputState>
    for AlsaReplacementBackend
{
    fn open_remixed(
        &mut self,
        request: AlsaRequest,
        mixer: NativeOutputState,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<
        AlsaReplacementOutput,
        OutputOpenFailure<AlsaReplacementError, AlsaReplacementOutput, NativeOutputState>,
    > {
        let native_matrix = match beatkernel::audio::ChannelMatrix::new(
            matrix.source_channels(),
            matrix.target_channels(),
            matrix.coefficients(),
        ) {
            Ok(copy) => copy,
            Err(error) => {
                return Err(OutputOpenFailure::recovered_state(
                    AlsaReplacementError::Linux(LinuxError::Mixer(error)),
                    Some(mixer),
                ));
            }
        };
        AlsaStream::open_state_remixed_recoverable(request, mixer, native_matrix)
            .map(|stream| AlsaReplacementOutput {
                stream,
                epoch,
                matrix: Some(matrix),
            })
            .map_err(|failure| {
                let (error, mixer) = failure.into_parts();
                OutputOpenFailure::recovered_state(AlsaReplacementError::Linux(error), mixer)
            })
    }
}

impl AlsaReplacementBackend {
    fn original_pair(
        &mut self,
        output: &mut AlsaReplacementOutput,
    ) -> Result<Option<beatkernel::time::ClockPair>, AlsaReplacementError> {
        Ok(self.original_coordinate(output)?.map(|(pair, _)| pair))
    }
    fn original_coordinate(
        &mut self,
        output: &mut AlsaReplacementOutput,
    ) -> Result<Option<(beatkernel::time::ClockPair, u64)>, AlsaReplacementError> {
        match output.stream.snapshot().status {
            AlsaStatus::Ready => return Ok(None),
            AlsaStatus::Running => {}
            status => return Err(AlsaReplacementError::Status(status)),
        }
        let Some(snapshot) = output.stream.timing_snapshot() else {
            return Ok(None);
        };
        alsa_presentation_pair_with_basis(snapshot, output.stream.frame_basis())
            .map(|pair| {
                pair.and_then(|pair| snapshot.estimated_played_frames.map(|frame| (pair, frame)))
            })
            .map_err(AlsaReplacementError::Linux)
    }
}

impl crate::gameplay::output::ports::OriginalNativeOutputBackend<NativeOutputState>
    for AlsaReplacementBackend
{
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error> {
        Ok(self.original_pair(output)?.map(|pair| crate::native_audio_presentation::NativeAudioSnapshot {
            epoch: output.epoch, basis: output.stream.frame_basis(),
            evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
        }))
    }
}

use crate::gameplay::output::ports::OriginalTargetNativeOutputBackend;
use beatkernel::audio::{ChannelMatrix, ConvertedRenderReport, TargetFrameBasis};
use beatkernel_platform::{
    audio::{ConvertedBoundaryFacts, ConvertedNativeOutputState},
    linux::{alsa_presentation_pair_with_target_basis, ConvertedAlsaStream},
};

/// Requested native target interpretation; omission retains the owner's actual matrix.
/// Source rate/channels always come from the complete software owner.
pub struct ConvertedAlsaReplacementRequest {
    pub native: AlsaRequest,
    pub matrix: Option<ChannelMatrix>,
}

/// Immutable creation identity and the matrix actually passed to the native worker.
pub struct ConvertedAlsaReplacementOutput {
    stream: ConvertedAlsaStream,
    epoch: u64,
    matrix: ChannelMatrix,
}
impl ConvertedAlsaReplacementOutput {
    pub fn stream(&self) -> &ConvertedAlsaStream {
        &self.stream
    }
    pub fn stream_mut(&mut self) -> &mut ConvertedAlsaStream {
        &mut self.stream
    }
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn channel_matrix(&self) -> &ChannelMatrix {
        &self.matrix
    }
}
impl StoppedMixerSource<ConvertedNativeOutputState> for ConvertedAlsaReplacementOutput {
    type Error = AlsaReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<ConvertedNativeOutputState>, Self::Error> {
        self.stream
            .take_stopped_output()
            .map_err(AlsaReplacementError::Linux)
    }
}

#[derive(Default)]
pub struct ConvertedAlsaReplacementBackend;
impl ConvertedAlsaReplacementBackend {
    fn planned_basis(
        request: &ConvertedAlsaReplacementRequest,
        owner: &ConvertedNativeOutputState,
    ) -> Result<TargetFrameBasis, AlsaReplacementError> {
        let native = &request.native;
        if native.device.is_empty()
            || native.device.contains('\0')
            || native.period_frames == 0
            || native.buffer_frames <= native.period_frames
            || native.format.channel_mask().is_some()
            || native.monotonic_domain == owner.mixer().config().domain()
        {
            return Err(AlsaReplacementError::Linux(LinuxError::InvalidConfiguration(
                "converted ALSA requires an explicit endpoint, distinct clocks, unspecified mask and 0 < period < buffer",
            )));
        }
        let matrix = request
            .matrix
            .as_ref()
            .unwrap_or_else(|| owner.converter_owner().converter().matrix());
        owner
            .validate_reconfigure(native.format, matrix, native.period_frames as usize)
            .map_err(|error| AlsaReplacementError::Linux(LinuxError::Mixer(error)))?;
        let basis = owner.target_frame_basis();
        TargetFrameBasis::new(
            basis.origin(),
            basis.start_time(),
            native.format.sample_rate(),
        )
        .map_err(|error| AlsaReplacementError::Linux(LinuxError::Mixer(error)))
    }
    fn copy_matrix(matrix: &ChannelMatrix) -> Result<ChannelMatrix, AlsaReplacementError> {
        ChannelMatrix::new(
            matrix.source_channels(),
            matrix.target_channels(),
            matrix.coefficients(),
        )
        .map_err(|error| AlsaReplacementError::Linux(LinuxError::Mixer(error)))
    }
    fn legacy_refusal() -> AlsaReplacementError {
        AlsaReplacementError::Linux(LinuxError::InvalidConfiguration(
            "converted ALSA requires original target-grid audio observations",
        ))
    }
    fn original_pair(
        &mut self,
        output: &mut ConvertedAlsaReplacementOutput,
    ) -> Result<Option<beatkernel::time::ClockPair>, AlsaReplacementError> {
        Ok(self.original_coordinate(output)?.map(|(pair, _)| pair))
    }
    fn original_coordinate(
        &mut self,
        output: &mut ConvertedAlsaReplacementOutput,
    ) -> Result<Option<(beatkernel::time::ClockPair, u64)>, AlsaReplacementError> {
        match output.stream.snapshot().status {
            AlsaStatus::Ready => return Ok(None),
            AlsaStatus::Running => {}
            status => return Err(AlsaReplacementError::Status(status)),
        }
        let Some(snapshot) = output.stream.timing_snapshot() else {
            return Ok(None);
        };
        alsa_presentation_pair_with_target_basis(snapshot, output.stream.frame_basis())
            .map(|pair| {
                pair.and_then(|pair| snapshot.estimated_played_frames.map(|frame| (pair, frame)))
            })
            .map_err(AlsaReplacementError::Linux)
    }
}
impl OutputReplacementBackend<ConvertedNativeOutputState, TargetFrameBasis>
    for ConvertedAlsaReplacementBackend
{
    type Presentation = PresentationDiscipline;
    type Output = ConvertedAlsaReplacementOutput;
    type Request = ConvertedAlsaReplacementRequest;
    type Error = AlsaReplacementError;
    fn open(
        &mut self,
        request: Self::Request,
        owner: ConvertedNativeOutputState,
        epoch: u64,
    ) -> Result<
        Self::Output,
        OutputOpenFailure<Self::Error, Self::Output, ConvertedNativeOutputState>,
    > {
        if let Err(error) = Self::planned_basis(&request, &owner) {
            return Err(OutputOpenFailure::recovered_state(error, Some(owner)));
        }
        let matrix = match request.matrix {
            Some(matrix) => matrix,
            None => match Self::copy_matrix(owner.converter_owner().converter().matrix()) {
                Ok(matrix) => matrix,
                Err(error) => return Err(OutputOpenFailure::recovered_state(error, Some(owner))),
            },
        };
        let native_matrix = match Self::copy_matrix(&matrix) {
            Ok(matrix) => matrix,
            Err(error) => return Err(OutputOpenFailure::recovered_state(error, Some(owner))),
        };
        ConvertedAlsaStream::open_recoverable(request.native, owner, native_matrix)
            .map(|stream| ConvertedAlsaReplacementOutput {
                stream,
                epoch,
                matrix,
            })
            .map_err(|failure| {
                let (error, owner) = failure.into_parts();
                OutputOpenFailure::recovered_state(AlsaReplacementError::Linux(error), owner)
            })
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.stop().map_err(AlsaReplacementError::Linux)
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.start().map_err(AlsaReplacementError::Linux)
    }
    fn epoch(&self, output: &Self::Output) -> u64 {
        output.epoch
    }
    fn basis(&self, output: &Self::Output) -> TargetFrameBasis {
        output.stream.frame_basis()
    }
    fn prepare_replacement_start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .stream
            .set_held(true)
            .map_err(AlsaReplacementError::Linux)
    }
    fn observe(
        &mut self,
        _: &mut Self::Output,
        _: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        Err(Self::legacy_refusal())
    }
    fn observe_replacement(
        &mut self,
        _: &mut Self::Output,
        _: &mut Self::Presentation,
        _: &crate::playback_pause::NativePause,
    ) -> Result<(), Self::Error> {
        Err(Self::legacy_refusal())
    }
    fn observe_end(
        &self,
        _: &Self::Output,
        _: &mut crate::native_end::NativeEnd,
        _: beatkernel::time::ClockPair,
        _: Option<RenderReport>,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<crate::native_end::EndBoundary>> {
        Err(Box::new(Self::legacy_refusal()))
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.stream.last_real_source_report())
    }
    fn pause_observation(
        &self,
        _: &Self::Output,
        _: beatkernel::time::ClockPair,
        _: beatkernel::time::ClockPoint,
    ) -> Result<crate::live_pause::LivePauseObservation, Self::Error> {
        Err(Self::legacy_refusal())
    }
}
impl OriginalTargetNativeOutputBackend<ConvertedNativeOutputState>
    for ConvertedAlsaReplacementBackend
{
    fn planned_target_basis(
        &self,
        request: &Self::Request,
        owner: &ConvertedNativeOutputState,
    ) -> Result<TargetFrameBasis, Self::Error> {
        Self::planned_basis(request, owner)
    }
    fn observe_native_target(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::TargetNativeAudioSnapshot>, Self::Error>
    {
        Ok(self.original_pair(output)?.map(|pair| crate::native_audio_presentation::TargetNativeAudioSnapshot {
            epoch: output.epoch,
            basis: output.stream.frame_basis(),
            evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
        }))
    }
    fn output_telemetry(
        &self,
        output: &Self::Output,
    ) -> Result<Option<crate::gameplay::output::ports::TargetOutputTelemetry>, Self::Error> {
        Ok(output
            .stream
            .output_telemetry()
            .map(|(source, converted, facts)| {
                crate::gameplay::output::ports::TargetOutputTelemetry {
                    source,
                    converted,
                    facts,
                }
            }))
    }
    fn converted_report(
        &self,
        output: &Self::Output,
    ) -> Result<Option<ConvertedRenderReport>, Self::Error> {
        Ok(output.stream.last_render_report())
    }
    fn boundary_facts(&self, output: &Self::Output) -> Result<ConvertedBoundaryFacts, Self::Error> {
        Ok(output.stream.boundary_facts())
    }
    fn set_held(&mut self, output: &mut Self::Output, held: bool) -> Result<(), Self::Error> {
        output
            .stream
            .set_held(held)
            .map_err(AlsaReplacementError::Linux)
    }
}

impl crate::gameplay::output::ports::OriginalCoordinateNativeOutputBackend<NativeOutputState>
    for AlsaReplacementBackend
{
    fn observe_native_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_output_coordinate::CoordinateNativeAudioSnapshot>, Self::Error>
    {
        use crate::native_output_coordinate::{CoordinateNativeAudioSnapshot, NativeOutputCoordinate};
        self.original_coordinate(output)?.map(|(pair, frame)| {
            CoordinateNativeAudioSnapshot::new(
                crate::native_audio_presentation::NativeAudioSnapshot {
                    epoch: output.epoch, basis: output.stream.frame_basis(),
                    evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
                }, NativeOutputCoordinate::StreamFrame { frame },
            ).map_err(AlsaReplacementError::Coordinate)
        }).transpose()
    }
}
impl
    crate::gameplay::output::ports::OriginalCoordinateTargetNativeOutputBackend<
        ConvertedNativeOutputState,
    > for ConvertedAlsaReplacementBackend
{
    fn observe_native_target_coordinate(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<
        Option<crate::native_output_coordinate::CoordinateTargetNativeAudioSnapshot>,
        Self::Error,
    > {
        use crate::native_output_coordinate::{
            CoordinateTargetNativeAudioSnapshot, NativeOutputCoordinate,
        };
        self.original_coordinate(output)?.map(|(pair, frame)| {
            CoordinateTargetNativeAudioSnapshot::new(
                crate::native_audio_presentation::TargetNativeAudioSnapshot {
                    epoch: output.epoch, basis: output.stream.frame_basis(),
                    evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
                }, NativeOutputCoordinate::StreamFrame { frame },
            ).map_err(AlsaReplacementError::Coordinate)
        }).transpose()
    }
}

use crate::gameplay::output::ports::{
    NativePlaybackObservation, NativeSourcePlaybackQuery, SourcePlaybackFacts,
    SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend, SourcePlaybackPacketError,
};
use beatkernel::audio::{InterpolatedTargetTime, TargetTime};
use beatkernel_platform::linux::{
    AlsaConvertedPlaybackSnapshot, AlsaSourcePlaybackSnapshot, AlsaTimingSnapshot,
};
use crate::gameplay::output::ports::{
    NativeTargetPlaybackFacts, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
    TargetHistoryRetirementNativeOutputBackend, TargetPlaybackNativeOutputBackend,
    TargetPlaybackPacketError,
};

/// Preserves the coherent converted publication and its original native evidence.
pub(super) fn qualify_alsa_target_packet(
    epoch: u64,
    basis: TargetFrameBasis,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    snapshot: AlsaConvertedPlaybackSnapshot,
) -> Result<NativeTargetPlaybackQuery<AlsaTimingSnapshot>, AlsaReplacementError> {
    let phase = snapshot.phase.phase;
    if phase.stream_start != basis.start_time() {
        return Err(AlsaReplacementError::TargetPlayback(
            TargetPlaybackPacketError::BasisMismatch,
        ));
    }
    let submitted_end = basis
        .time_at_stream_frame(snapshot.submitted_target_frames)
        .map_err(|_| AlsaReplacementError::TargetPlayback(TargetPlaybackPacketError::Overflow))?;
    if snapshot.submitted_target_frames == 0
        || phase.submitted_end != submitted_end
        || snapshot
            .timing
            .is_some_and(|timing| timing.submitted_frames != snapshot.submitted_target_frames)
    {
        return Err(AlsaReplacementError::TargetPlayback(
            TargetPlaybackPacketError::InvalidMapping(
                crate::native_playback_phase::NativePlaybackPhaseError::Range,
            ),
        ));
    }
    NativeTargetPlaybackQuery::new(
        epoch,
        basis,
        identity,
        query,
        NativeTargetPlaybackFacts {
            playback: crate::native_playback_phase::ConvertedPlaybackFacts {
                phase: phase.span,
                segment_end: phase.segment_end,
                generated_target_end: phase.generated_end,
                submitted_target_end: phase.submitted_end,
                playback: snapshot.phase.playback,
                pulled_source_end: snapshot.phase.pulled_source_end,
                revision: phase.revision,
            },
            source: phase.last_real_source,
            converted: phase.report,
            boundaries: phase.boundaries,
            revision: snapshot.revision,
        },
        snapshot.timing,
    )
    .map_err(AlsaReplacementError::TargetPlayback)
}

/// The candidate chooses the query; only identical coherent evidence supplies a pair.
pub(super) fn qualify_alsa_target_observation_packet(
    epoch: u64,
    basis: TargetFrameBasis,
    identity: SourcePlaybackIdentity,
    candidate: AlsaTimingSnapshot,
    packet: NativeTargetPlaybackQuery<AlsaTimingSnapshot>,
) -> Result<Option<NativeTargetPlaybackObservation<AlsaTimingSnapshot>>, AlsaReplacementError> {
    if packet.native_evidence() != Some(&candidate) {
        return Ok(None);
    }
    if packet.epoch() != epoch {
        return Err(AlsaReplacementError::TargetPlayback(
            TargetPlaybackPacketError::EpochMismatch,
        ));
    }
    if packet.basis() != basis || packet.identity() != identity {
        return Err(AlsaReplacementError::TargetPlayback(
            TargetPlaybackPacketError::BasisMismatch,
        ));
    }
    let Some(pair) = alsa_presentation_pair_with_target_basis(candidate, basis)
        .map_err(AlsaReplacementError::Linux)?
    else {
        return Ok(None);
    };
    let Some(frame) = candidate.estimated_played_frames else {
        return Ok(None);
    };
    let coordinate = crate::native_output_coordinate::CoordinateTargetNativeAudioSnapshot::new(
        crate::native_audio_presentation::TargetNativeAudioSnapshot {
            epoch, basis,
            evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
        },
        crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame },
    ).map_err(AlsaReplacementError::Coordinate)?;
    NativeTargetPlaybackObservation::new(coordinate, packet)
        .map(Some)
        .map_err(AlsaReplacementError::TargetPlayback)
}

impl TargetPlaybackNativeOutputBackend<ConvertedNativeOutputState>
    for ConvertedAlsaReplacementBackend
{
    type NativeEvidence = AlsaTimingSnapshot;
    fn target_playback_at(
        &self,
        output: &Self::Output,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackQuery<Self::NativeEvidence>>, Self::Error> {
        output
            .stream
            .native_playback_at(
                query,
                identity.hz(),
                identity.initial_physical(),
                identity.initial_playback(),
            )
            .map(|snapshot| {
                qualify_alsa_target_packet(
                    output.epoch,
                    output.stream.frame_basis(),
                    identity,
                    *query,
                    snapshot,
                )
            })
            .transpose()
    }
    fn observe_native_target_playback(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackObservation<Self::NativeEvidence>>, Self::Error> {
        match output.stream.snapshot().status {
            AlsaStatus::Ready => return Ok(None),
            AlsaStatus::Running => {}
            status => return Err(AlsaReplacementError::Status(status)),
        }
        let basis = output.stream.frame_basis();
        for _ in 0..3 {
            let Some(candidate) = output.stream.timing_snapshot() else {
                continue;
            };
            if alsa_presentation_pair_with_target_basis(candidate, basis)
                .map_err(AlsaReplacementError::Linux)?
                .is_none()
            {
                continue;
            }
            let Some(frame) = candidate.estimated_played_frames else {
                continue;
            };
            let query = InterpolatedTargetTime::new(
                basis.start_time(),
                frame,
                frame,
                u64::from(basis.sample_rate()),
                0,
                1,
            )
            .map_err(|_| {
                AlsaReplacementError::TargetPlayback(TargetPlaybackPacketError::Overflow)
            })?;
            let Some(packet) = self.target_playback_at(output, &query, identity)? else {
                continue;
            };
            if let Some(observation) = qualify_alsa_target_observation_packet(
                output.epoch,
                basis,
                identity,
                candidate,
                packet,
            )? {
                return Ok(Some(observation));
            }
        }
        Ok(None)
    }
}

impl
    crate::gameplay::output::ports::ConditionalTargetRetirementNativeOutputBackend<
        ConvertedNativeOutputState,
    > for ConvertedAlsaReplacementBackend
{
    type Ticket = beatkernel_platform::linux::ConvertedRetirementTicket;
    fn submit_target_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        request: &Self::Request,
    ) -> Result<Option<Self::Ticket>, Self::Error> {
        if request.native.monotonic_domain
            != output.stream.configuration().requested.monotonic_domain
        {
            return Err(AlsaReplacementError::Linux(
                LinuxError::InvalidConfiguration(
                    "converted replacement must retain the original native HOST domain",
                ),
            ));
        }
        let matrix = request.matrix.as_ref().unwrap_or(&output.matrix);
        output
            .stream
            .validate_conditional_replacement(&request.native, matrix)
            .map_err(AlsaReplacementError::Linux)?;
        let policy = beatkernel_platform::linux::ConvertedRetirementPolicy::for_interpretation(
            output.stream.configuration().format,
            &output.matrix,
            request.native.format,
            matrix,
        );
        output
            .stream
            .submit_conditional_retirement(policy)
            .map_err(AlsaReplacementError::Linux)
    }
    fn poll_target_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<crate::gameplay::output::ports::TargetRetirementStatus, Self::Error> {
        output
            .stream
            .poll_conditional_retirement(ticket)
            .map(target_retirement_status)
            .map_err(AlsaReplacementError::Linux)
    }
    fn cancel_target_conditional_retirement(
        &mut self,
        output: &mut Self::Output,
        ticket: &Self::Ticket,
    ) -> Result<crate::gameplay::output::ports::TargetRetirementStatus, Self::Error> {
        output
            .stream
            .cancel_conditional_retirement(ticket)
            .map(target_retirement_status)
            .map_err(AlsaReplacementError::Linux)
    }
}
fn target_retirement_status(
    status: beatkernel_platform::linux::ConvertedRetirementStatus,
) -> crate::gameplay::output::ports::TargetRetirementStatus {
    use crate::gameplay::output::ports::TargetRetirementStatus as Status;
    use beatkernel_platform::linux::ConvertedRetirementStatus as Native;
    match status {
        Native::Pending => Status::Pending,
        Native::Refused => Status::Refused,
        Native::Cancelled => Status::Cancelled,
        Native::Authorized => Status::Authorized,
    }
}

impl TargetHistoryRetirementNativeOutputBackend<ConvertedNativeOutputState>
    for ConvertedAlsaReplacementBackend
{
    fn request_target_history_retirement(
        &mut self,
        output: &mut Self::Output,
        expected_epoch: u64,
        expected_basis: TargetFrameBasis,
        identity: SourcePlaybackIdentity,
        source_pin: u64,
        target_pin: TargetTime,
    ) -> Result<Option<u64>, Self::Error> {
        if expected_epoch != output.epoch {
            return Err(AlsaReplacementError::EpochMismatch);
        }
        if expected_basis != output.stream.frame_basis() {
            return Err(AlsaReplacementError::TargetPlayback(
                TargetPlaybackPacketError::BasisMismatch,
            ));
        }
        output
            .stream
            .request_history_retirement(
                expected_basis,
                (
                    identity.hz(),
                    identity.initial_physical(),
                    identity.initial_playback(),
                ),
                source_pin,
                target_pin,
            )
            .map_err(AlsaReplacementError::Linux)
    }
}

/// Qualifies only the mathematical source packet, retaining optional raw evidence.
/// Native status/freshness/epoch admission remains outside this read-only boundary.
pub(super) fn qualify_alsa_source_packet(
    epoch: u64,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    query: InterpolatedTargetTime,
    snapshot: AlsaSourcePlaybackSnapshot,
) -> Result<NativeSourcePlaybackQuery<AlsaTimingSnapshot>, AlsaReplacementError> {
    if snapshot.timing.is_some_and(|timing| {
        basis
            .start_physical_frame()
            .checked_add(timing.submitted_frames)
            != Some(snapshot.submitted_source_end)
    }) {
        return Err(AlsaReplacementError::Playback(
            SourcePlaybackPacketError::InvalidPrefix,
        ));
    }
    NativeSourcePlaybackQuery::new(
        epoch,
        basis,
        identity,
        query,
        SourcePlaybackFacts {
            playback: snapshot.playback,
            pulled_source_end: snapshot.pulled_source_end,
            submitted_source_end: snapshot.submitted_source_end,
            render: snapshot.render,
            revision: snapshot.revision,
        },
        snapshot.timing,
    )
    .map_err(AlsaReplacementError::Playback)
}

/// The full native candidate must be the evidence in the compound source read.
/// A changing or absent publication yields no observation, never a mixed pair.
pub(super) fn qualify_alsa_observation_packet(
    epoch: u64,
    basis: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    candidate: AlsaTimingSnapshot,
    packet: NativeSourcePlaybackQuery<AlsaTimingSnapshot>,
) -> Result<Option<NativePlaybackObservation<AlsaTimingSnapshot>>, AlsaReplacementError> {
    if packet.native_evidence() != Some(&candidate) {
        return Ok(None);
    }
    if packet.epoch() != epoch {
        return Err(AlsaReplacementError::Playback(
            SourcePlaybackPacketError::EpochMismatch,
        ));
    }
    if packet.basis() != basis || packet.identity() != identity {
        return Err(AlsaReplacementError::Playback(
            SourcePlaybackPacketError::BasisMismatch,
        ));
    }
    let Some(pair) =
        alsa_presentation_pair_with_basis(candidate, basis).map_err(AlsaReplacementError::Linux)?
    else {
        return Ok(None);
    };
    let Some(frame) = candidate.estimated_played_frames else {
        return Ok(None);
    };
    let coordinate = crate::native_output_coordinate::CoordinateNativeAudioSnapshot::new(
        crate::native_audio_presentation::NativeAudioSnapshot {
            epoch, basis,
            evidence: beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence::SuppliedPair(pair),
        }, crate::native_output_coordinate::NativeOutputCoordinate::StreamFrame { frame },
    ).map_err(AlsaReplacementError::Coordinate)?;
    NativePlaybackObservation::new(coordinate, packet)
        .map(Some)
        .map_err(AlsaReplacementError::Playback)
}

impl SourcePlaybackNativeOutputBackend<NativeOutputState> for AlsaReplacementBackend {
    type NativeEvidence = AlsaTimingSnapshot;
    fn source_playback_at(
        &self,
        output: &Self::Output,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<Self::NativeEvidence>>, Self::Error> {
        let basis = output.stream.frame_basis();
        if basis.sample_rate() != identity.hz()
            || identity.initial_physical() > basis.start_physical_frame()
        {
            return Err(AlsaReplacementError::Playback(
                SourcePlaybackPacketError::BasisMismatch,
            ));
        }
        output
            .stream
            .source_playback_at(
                query,
                identity.hz(),
                identity.initial_physical(),
                identity.initial_playback(),
            )
            .map(|snapshot| {
                qualify_alsa_source_packet(output.epoch, basis, identity, *query, snapshot)
            })
            .transpose()
    }
    fn observe_native_playback(
        &mut self,
        output: &mut Self::Output,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<Self::NativeEvidence>>, Self::Error> {
        match output.stream.snapshot().status {
            AlsaStatus::Ready => return Ok(None),
            AlsaStatus::Running => {}
            status => return Err(AlsaReplacementError::Status(status)),
        }
        let basis = output.stream.frame_basis();
        for _ in 0..3 {
            let Some(candidate) = output.stream.timing_snapshot() else {
                continue;
            };
            // Check native metadata before constructing its mathematical query.
            if alsa_presentation_pair_with_basis(candidate, basis)
                .map_err(AlsaReplacementError::Linux)?
                .is_none()
            {
                continue;
            }
            let Some(frame) = candidate.estimated_played_frames else {
                continue;
            };
            let base = TargetTime::from_frames(basis.start_physical_frame(), basis.sample_rate())
                .map_err(|_| {
                AlsaReplacementError::Playback(SourcePlaybackPacketError::Overflow)
            })?;
            let query = InterpolatedTargetTime::new(
                base,
                frame,
                frame,
                u64::from(basis.sample_rate()),
                0,
                1,
            )
            .map_err(|_| AlsaReplacementError::Playback(SourcePlaybackPacketError::Overflow))?;
            let Some(packet) = self.source_playback_at(output, &query, identity)? else {
                continue;
            };
            if let Some(observation) =
                qualify_alsa_observation_packet(output.epoch, basis, identity, candidate, packet)?
            {
                return Ok(Some(observation));
            }
        }
        Ok(None)
    }
}
