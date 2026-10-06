//! Static channel-request routing through unchanged output ownership.
use crate::{
    gameplay::output::{
        domain::remix::RemixedOutputRequest,
        ports::{OutputChannelRemixBackend, OutputReplacementBackend},
    },
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::NativeGameplayResult,
};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport},
    time::{ClockPair, ClockPoint},
};

pub struct RemixedOutputBackend<B> {
    inner: B,
}
impl<B> RemixedOutputBackend<B> {
    pub const fn new(inner: B) -> Self {
        Self { inner }
    }
}
impl<B: OutputChannelRemixBackend> OutputReplacementBackend for RemixedOutputBackend<B> {
    type Presentation = B::Presentation;
    type Output = B::Output;
    type Request = RemixedOutputRequest<B::Request>;
    type Error = B::Error;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        match request.matrix {
            Some(matrix) => self
                .inner
                .open_remixed(request.native, mixer, epoch, matrix),
            None => self.inner.open(request.native, mixer, epoch),
        }
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        self.inner.retire(output)
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        self.inner.start(output)
    }
    fn epoch(&self, output: &Self::Output) -> u64 {
        self.inner.epoch(output)
    }
    fn basis(&self, output: &Self::Output) -> OutputFrameBasis {
        self.inner.basis(output)
    }
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        self.inner.observe(output, presentation)
    }
    fn observe_end(
        &self,
        output: &Self::Output,
        end: &mut NativeEnd,
        pair: ClockPair,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        self.inner.observe_end(output, end, pair, report)
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        self.inner.render_report(output)
    }
    fn pause_observation(
        &self,
        output: &Self::Output,
        pair: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, Self::Error> {
        self.inner.pause_observation(output, pair, now)
    }
}

#[cfg(test)]
#[path = "remix_fixtures.rs"]
mod remix_fixtures;
