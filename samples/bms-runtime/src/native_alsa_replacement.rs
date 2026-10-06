//! Actual ALSA lifecycle adapter for the portable paused-output replacement policy.
use crate::output_replacement::OutputReplacementBackend;
use beatkernel::audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineError, PresentationDiscipline},
    linux::{AlsaRequest, AlsaStatus, AlsaStream, LinuxError, alsa_presentation_pair_with_basis},
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
}
impl AlsaReplacementOutput {
    pub fn from_stream(stream: AlsaStream) -> Self {
        Self { stream, epoch: 0 }
    }
    pub fn stream(&self) -> &AlsaStream {
        &self.stream
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
impl StoppedMixerSource for AlsaReplacementOutput {
    type Error = AlsaReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        self.stream
            .take_stopped_mixer()
            .map_err(AlsaReplacementError::Linux)
    }
}
#[derive(Default)]
pub struct AlsaReplacementBackend;
impl OutputReplacementBackend for AlsaReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = AlsaReplacementOutput;
    type Request = AlsaRequest;
    type Error = AlsaReplacementError;
    fn open(
        &mut self,
        request: AlsaRequest,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        AlsaStream::open_recoverable(request, mixer)
            .map(|stream| AlsaReplacementOutput { stream, epoch })
            .map_err(|failure| {
                let (error, mixer) = failure.into_parts();
                OutputOpenFailure::recovered(AlsaReplacementError::Linux(error), mixer)
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
        match output.stream.snapshot().status {
            AlsaStatus::Ready => return Ok(()),
            AlsaStatus::Running => {}
            status => return Err(AlsaReplacementError::Status(status)),
        }
        let Some(snapshot) = output.stream.timing_snapshot() else {
            return Ok(());
        };
        if let Some(pair) = alsa_presentation_pair_with_basis(snapshot, output.stream.frame_basis())
            .map_err(AlsaReplacementError::Linux)?
        {
            presentation
                .observe_clock_pair_in_epoch(output.epoch, pair)
                .map_err(AlsaReplacementError::Discipline)?;
        }
        Ok(())
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.stream.last_render_report())
    }
}
