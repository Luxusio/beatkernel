//! Static effect contracts for gameplay output.
use super::domain::control::{OutputCapability, OutputReply, OutputRequest};
use crate::{gameplay_presentation::GameplayPresentationPort, live_pause::LivePauseObservation};
use beatkernel::{
    audio::{
        Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, SoftwareOutputState,
        StoppedMixerSource,
    },
    time::{ClockPair, ClockPoint},
};
use std::io;

/// Lifecycle ownership does not imply a fixed source-grid timing capability.
/// Converted backends use `TargetFrameBasis` and retain their complete owner.
pub trait OutputReplacementBackend<O = Mixer, Basis = OutputFrameBasis> {
    type Presentation: GameplayPresentationPort;
    type Output: StoppedMixerSource<O, Error = Self::Error>;
    type Request;
    type Error;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: O,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output, O>>;
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error>;
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error>;
    /// Cold setup immediately before a paused replacement starts. Initial
    /// stream startup does not invoke this hook; source-grid backends need no setup.
    fn prepare_replacement_start(&mut self, _: &mut Self::Output) -> Result<(), Self::Error> {
        Ok(())
    }
    fn epoch(&self, output: &Self::Output) -> u64;
    fn basis(&self, output: &Self::Output) -> Basis;
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error>;
    /// Adapter may defer native pairs preceding the retained-tail pause frontier.
    fn observe_replacement(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
        _: &crate::playback_pause::NativePause,
    ) -> Result<(), Self::Error> {
        self.observe(output, presentation)
    }
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
pub trait OriginalNativeOutputBackend<O: SoftwareOutputState = Mixer>:
    OutputReplacementBackend<O>
{
    fn observe_native(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::NativeAudioSnapshot>, Self::Error>;
}

/// Original observations for a target-time output, independent of source pull.
/// Native lifecycle is shared; this port adds no alternative clock discipline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetOutputTelemetry {
    pub source: Option<RenderReport>,
    pub converted: Option<beatkernel::audio::ConvertedRenderReport>,
    pub facts: beatkernel_platform::audio::ConvertedBoundaryFacts,
}

pub trait OriginalTargetNativeOutputBackend<O>:
    OutputReplacementBackend<O, beatkernel::audio::TargetFrameBasis>
{
    /// Validate pending interpretation and plan the new immutable creation basis
    /// before transferring the recovered owner to the native worker.
    fn planned_target_basis(
        &self,
        request: &Self::Request,
        owner: &O,
    ) -> Result<beatkernel::audio::TargetFrameBasis, Self::Error>;
    fn observe_native_target(
        &mut self,
        output: &mut Self::Output,
    ) -> Result<Option<crate::native_audio_presentation::TargetNativeAudioSnapshot>, Self::Error>;
    fn converted_report(
        &self,
        output: &Self::Output,
    ) -> Result<Option<beatkernel::audio::ConvertedRenderReport>, Self::Error>;
    fn boundary_facts(
        &self,
        output: &Self::Output,
    ) -> Result<beatkernel_platform::audio::ConvertedBoundaryFacts, Self::Error>;
    /// The default is suitable only for serialized same-owner implementations.
    /// Concurrent worker adapters must override with one coherent bounded read.
    fn output_telemetry(
        &self,
        output: &Self::Output,
    ) -> Result<Option<TargetOutputTelemetry>, Self::Error> {
        let source = self.render_report(output)?;
        let converted = self.converted_report(output)?;
        let facts = self.boundary_facts(output)?;
        Ok(
            (facts != beatkernel_platform::audio::ConvertedBoundaryFacts::default()).then_some(
                TargetOutputTelemetry {
                    source,
                    converted,
                    facts,
                },
            ),
        )
    }
    /// Paused replacements select held output before native `start`; ordinary
    /// transitions still wait for actual source adoption and native crossing.
    fn set_held(&mut self, output: &mut Self::Output, held: bool) -> Result<(), Self::Error>;
}

/// Explicit same-rate channel conversion; no default can silently ignore a matrix.
pub trait OutputChannelRemixBackend<O: SoftwareOutputState = Mixer>:
    OutputReplacementBackend<O>
{
    fn open_remixed(
        &mut self,
        request: Self::Request,
        mixer: O,
        epoch: u64,
        matrix: beatkernel::audio::ChannelMatrix,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output, O>>;
}

pub trait OutputUiPort {
    fn advertise(&mut self, capability: Option<OutputCapability>) -> io::Result<()>;
    fn take_request(&mut self) -> io::Result<Option<OutputRequest>>;
    fn reply(&mut self, reply: &OutputReply) -> io::Result<()>;
    fn pending(&self) -> bool;
}
