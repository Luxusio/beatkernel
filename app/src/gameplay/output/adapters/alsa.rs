//! Actual ALSA lifecycle adapter for the portable paused-output replacement policy.
use crate::gameplay::output::ports::OutputReplacementBackend;
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
