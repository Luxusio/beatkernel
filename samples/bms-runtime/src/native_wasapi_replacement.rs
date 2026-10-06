//! WASAPI lifecycle adapter; all observed counters retain their creation epoch.
use crate::{
    output_replacement::OutputReplacementBackend,
    native_replacement_observation::{observe_wasapi, ReplacementObservationError},
};
use beatkernel::audio::{
    Mixer, MixerOpenFailure, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource,
};
use beatkernel_platform::{
    audio::{
        AudioOutputStream, AudioPlatformError, AudioStreamRequest,
        presentation::discipline::PresentationDiscipline,
    },
    windows::{
        audio::{WasapiBackend, WasapiOptions, WasapiStream},
        clock::QpcClock,
    },
};
#[derive(Debug)]
pub enum WasapiReplacementError {
    Native(AudioPlatformError),
    Observation(ReplacementObservationError),
}
impl std::fmt::Display for WasapiReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(e) => e.fmt(f),
            Self::Observation(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for WasapiReplacementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Native(e) => Some(e),
            Self::Observation(e) => Some(e),
        }
    }
}
pub struct WasapiReplacementOutput {
    stream: WasapiStream,
    epoch: u64,
}
impl WasapiReplacementOutput {
    pub fn from_stream(stream: WasapiStream) -> Self {
        Self { stream, epoch: 0 }
    }
    pub fn stream(&self) -> &WasapiStream {
        &self.stream
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}
impl StoppedMixerSource for WasapiReplacementOutput {
    type Error = WasapiReplacementError;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        self.stream
            .take_stopped_mixer()
            .map_err(WasapiReplacementError::Native)
    }
}
fn map_open_failure(
    failure: MixerOpenFailure<AudioPlatformError>,
) -> OutputOpenFailure<WasapiReplacementError, WasapiReplacementOutput> {
    let (error, mixer) = failure.into_parts();
    OutputOpenFailure::recovered(WasapiReplacementError::Native(error), mixer)
}
pub struct WasapiReplacementBackend {
    clock: QpcClock,
    options: WasapiOptions,
}
impl WasapiReplacementBackend {
    pub fn new(clock: QpcClock, options: WasapiOptions) -> Self {
        Self { clock, options }
    }
}
impl OutputReplacementBackend for WasapiReplacementBackend {
    type Presentation = PresentationDiscipline;
    type Output = WasapiReplacementOutput;
    type Request = AudioStreamRequest;
    type Error = WasapiReplacementError;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        WasapiBackend
            .open_recoverable(request, mixer, self.clock, self.options)
            .map(|stream| WasapiReplacementOutput { stream, epoch })
            .map_err(map_open_failure)
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output.stream.stop().map_err(WasapiReplacementError::Native)
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        output
            .stream
            .start()
            .map_err(WasapiReplacementError::Native)
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
        observe_wasapi(
            presentation,
            output.epoch,
            output.stream.snapshot(),
            output.stream.frame_basis(),
        )
        .map(|_| ())
        .map_err(WasapiReplacementError::Observation)
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        Ok(output.stream.snapshot().render)
    }
}
#[cfg(test)]
#[path = "native_wasapi_replacement_fixtures.rs"]
mod fixtures;
