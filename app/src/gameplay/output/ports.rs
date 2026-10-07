//! Static effect contracts for gameplay output.
use crate::{gameplay_presentation::GameplayPresentationPort, live_pause::LivePauseObservation};
use super::domain::control::{OutputCapability, OutputRequest, OutputReply};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    time::{ClockPair, ClockPoint},
};
use std::io;

pub trait OutputReplacementBackend {
    type Presentation: GameplayPresentationPort;
    type Output: StoppedMixerSource<Error = Self::Error>;
    type Request;
    type Error;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>>;
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error>;
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error>;
    fn epoch(&self, output: &Self::Output) -> u64;
    fn basis(&self, output: &Self::Output) -> OutputFrameBasis;
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error>;
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
pub trait OriginalNativeOutputBackend: OutputReplacementBackend {
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error>;
}

/// Explicit same-rate channel conversion; no default can silently ignore a matrix.
pub trait OutputChannelRemixBackend: OutputReplacementBackend {
    fn open_remixed(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>>;
}

pub trait OutputUiPort {
    fn advertise(&mut self, capability: Option<OutputCapability>) -> io::Result<()>;
    fn take_request(&mut self) -> io::Result<Option<OutputRequest>>;
    fn reply(&mut self, reply: &OutputReply) -> io::Result<()>;
    fn pending(&self) -> bool;
}
