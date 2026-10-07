//! CoreAudio replacement retains actual callback owners and thread affinity.
use crate::gameplay::output::ports::OutputReplacementBackend;
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    time::ClockDomainId,
};
use beatkernel_platform::{
    audio::presentation::discipline::{DisciplineError, PresentationDiscipline},
    macos::{
        audio::{CoreAudioError, CoreAudioOpenFailure, CoreAudioRequest, CoreAudioStream},
        clock::MachClock,
        presentation::{CoreAudioPresentationError, coreaudio_presentation_pair},
    },
};
#[derive(Debug)]
pub enum CoreAudioReplacementError {
    Native(CoreAudioError),
    Presentation(CoreAudioPresentationError),
    Discipline(DisciplineError),
    CallbackFailure(u64),
    EpochMismatch,
}
impl std::fmt::Display for CoreAudioReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(e) => e.fmt(f),
            Self::Presentation(e) => e.fmt(f),
            Self::Discipline(e) => e.fmt(f),
            Self::CallbackFailure(count) => {
                write!(f, "CoreAudio replacement callback failures: {count}")
            }
            Self::EpochMismatch => f.write_str("CoreAudio replacement observation epoch differs"),
        }
    }
}
impl std::error::Error for CoreAudioReplacementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Native(e) => Some(e),
            Self::Presentation(e) => Some(e),
            Self::Discipline(e) => Some(e),
            _ => None,
        }
    }
}
pub struct CoreAudioReplacementOutput {
    stream: CoreAudioStream,
    epoch: u64,
    matrix: Option<beatkernel::audio::ChannelMatrix>,
}
impl CoreAudioReplacementOutput {
    pub fn from_stream(stream: CoreAudioStream) -> Self {
        Self {
            stream,
            epoch: 0,
            matrix: None,
        }
    }
    pub fn stream(&self) -> &CoreAudioStream {
        &self.stream
    }
    pub fn stream_mut(&mut self) -> &mut CoreAudioStream {
        &mut self.stream
    }
    pub fn channel_matrix(&self) -> Option<&beatkernel::audio::ChannelMatrix> {
        self.matrix.as_ref()
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
impl StoppedMixerSource for CoreAudioReplacementOutput {
    type Error = CoreAudioReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        self.stream
            .take_stopped_mixer()
            .map_err(CoreAudioReplacementError::Native)
    }
}
fn map_open_failure(
    failure: CoreAudioOpenFailure,
    epoch: u64,
) -> OutputOpenFailure<CoreAudioReplacementError, CoreAudioReplacementOutput> {
    let (error, mixer, pending, cleanup) = failure.into_parts();
    let mapped = match pending {
        Some(stream) => OutputOpenFailure::pending(
            CoreAudioReplacementError::Native(error),
            CoreAudioReplacementOutput {
                stream,
                epoch,
                matrix: None,
            },
        ),
        None => OutputOpenFailure::recovered(CoreAudioReplacementError::Native(error), mixer),
    };
    match cleanup {
        Some(error) => mapped.with_cleanup_error(CoreAudioReplacementError::Native(error)),
        None => mapped,
    }
}
pub struct CoreAudioReplacementBackend {
    clock: MachClock,
    host_domain: ClockDomainId,
}
impl CoreAudioReplacementBackend {
    pub fn new(clock: MachClock, host_domain: ClockDomainId) -> Self {
        Self { clock, host_domain }
    }
}
impl OutputReplacementBackend for CoreAudioReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = CoreAudioReplacementOutput;
    type Request = CoreAudioRequest;
    type Error = CoreAudioReplacementError;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        CoreAudioStream::open_recoverable(request, self.clock, mixer)
            .map(|stream| CoreAudioReplacementOutput {
                stream,
                epoch,
                matrix: None,
            })
            .map_err(|error| map_open_failure(error, epoch))
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .stream
            .stop()
            .map_err(CoreAudioReplacementError::Native)?;
        Ok(())
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .stream
            .start()
            .map_err(CoreAudioReplacementError::Native)?;
        Ok(())
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
            return Err(CoreAudioReplacementError::EpochMismatch);
        }
        let snapshot = output.stream.snapshot();
        if snapshot.configuration_changed {
            return Err(CoreAudioReplacementError::Native(
                CoreAudioError::ConfigurationChanged,
            ));
        }
        if snapshot.callback_failures != 0 {
            return Err(CoreAudioReplacementError::CallbackFailure(
                snapshot.callback_failures,
            ));
        }
        if !output.stream.is_started() {
            return Ok(());
        }
        let Some(observation) = snapshot.presentation else {
            return Ok(());
        };
        // CoreAudio observations already carry absolute original-grid frames.
        if let Some(pair) = coreaudio_presentation_pair(
            observation,
            output.stream.configuration(),
            self.host_domain,
            &self.clock,
        )
        .map_err(CoreAudioReplacementError::Presentation)?
        {
            presentation
                .observe_clock_pair_in_epoch(output.epoch, pair)
                .map_err(CoreAudioReplacementError::Discipline)?;
        }
        Ok(())
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.stream.last_render_report())
    }
}

impl crate::gameplay::output::ports::OutputChannelRemixBackend for CoreAudioReplacementBackend {
    fn open_remixed(
        &mut self,
        request: CoreAudioRequest,
        mixer: Mixer,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<
        CoreAudioReplacementOutput,
        OutputOpenFailure<CoreAudioReplacementError, CoreAudioReplacementOutput>,
    > {
        let native_matrix = match beatkernel::audio::ChannelMatrix::new(
            matrix.source_channels(),
            matrix.target_channels(),
            matrix.coefficients(),
        ) {
            Ok(copy) => copy,
            Err(_) => {
                return Err(OutputOpenFailure::recovered(
                    CoreAudioReplacementError::Native(CoreAudioError::Capacity),
                    Some(mixer),
                ));
            }
        };
        CoreAudioStream::open_remixed_recoverable(request, self.clock, mixer, native_matrix)
            .map(|stream| CoreAudioReplacementOutput {
                stream,
                epoch,
                matrix: Some(matrix),
            })
            .map_err(|failure| map_open_failure(failure, epoch))
    }
}
#[cfg(test)]
#[path = "coreaudio_fixtures.rs"]
mod fixtures;
