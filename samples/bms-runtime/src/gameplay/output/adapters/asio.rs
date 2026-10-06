//! Trusted same-thread ASIO acquisition and original interval evidence.
use crate::{
    gameplay::output::ports::OutputReplacementBackend,
    gameplay::output::adapters::observation::{
        observe_asio_with_basis, asio_pause_observation, ReplacementObservationError,
    },
    live_pause::LivePauseObservation,
};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    time::{ClockPair, ClockPoint},
};
use beatkernel_platform::{
    audio::{
        asio::{
            AsioBufferRequest, AsioPresentationObservation, AsioPresentationError,
            MultimediaClockAnchor,
        },
        presentation::discipline::PresentationDiscipline,
    },
    windows::{
        clock::QpcClock,
        asio::{
            control::TrustedAsioOpener,
            stream::{AsioStream, AsioStreamError, AsioStreamPhase, AsioPrepareFailure},
        },
    },
};
#[derive(Debug)]
pub enum AsioReplacementError {
    Native(AsioStreamError),
    Host(std::io::Error),
    Presentation(AsioPresentationError),
    Observation(ReplacementObservationError),
    Status(AsioStreamPhase),
}
impl std::fmt::Display for AsioReplacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(e) => e.fmt(f),
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
            Self::Native(e) => Some(e),
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
            AsioStreamPhase::Ready => return Ok(()),
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
                return Ok(());
            }
            Err(error) => return Err(AsioReplacementError::Presentation(error)),
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
        Ok(asio_pause_observation(output.observation, now))
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
