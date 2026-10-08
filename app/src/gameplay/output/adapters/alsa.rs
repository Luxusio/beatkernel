//! Actual ALSA lifecycle adapter for the portable paused-output replacement policy.
use crate::gameplay::output::ports::OutputReplacementBackend;
#[cfg(test)]
#[path = "alsa_target_fixtures.rs"]
mod target_fixtures;
use beatkernel::audio::{OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineError, PresentationDiscipline},
    audio::NativeOutputState,
    linux::{alsa_presentation_pair_with_basis, AlsaRequest, AlsaStatus, AlsaStream, LinuxError},
};

#[derive(Debug)]
pub enum AlsaReplacementError {
    Linux(LinuxError),
    Discipline(DisciplineError),
    Status(AlsaStatus),
    EpochMismatch,
}
impl std::fmt::Display for AlsaReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
        match output.stream.snapshot().status {
            AlsaStatus::Ready => return Ok(None),
            AlsaStatus::Running => {}
            status => return Err(AlsaReplacementError::Status(status)),
        }
        let Some(snapshot) = output.stream.timing_snapshot() else {
            return Ok(None);
        };
        alsa_presentation_pair_with_basis(snapshot, output.stream.frame_basis())
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
        match output.stream.snapshot().status {
            AlsaStatus::Ready => return Ok(None),
            AlsaStatus::Running => {}
            status => return Err(AlsaReplacementError::Status(status)),
        }
        let Some(snapshot) = output.stream.timing_snapshot() else {
            return Ok(None);
        };
        alsa_presentation_pair_with_target_basis(snapshot, output.stream.frame_basis())
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
