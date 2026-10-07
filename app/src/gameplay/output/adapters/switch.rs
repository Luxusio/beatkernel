//! Static two-backend output adapter; the common owner keeps one replacement policy.
//!
//! Each side keeps its own typed output, request and error. A request selects the
//! opening side; every other lifecycle call follows the side that owns the output,
//! so the recovered Mixer moves between backends through the unchanged controller.
use crate::{
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::NativeGameplayResult,
    gameplay::output::ports::OutputReplacementBackend,
};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, OutputOpenFailure, RenderReport, StoppedMixerSource},
    time::{ClockPair, ClockPoint},
};

/// Output, request or error owned by exactly one side of an output switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switched<A, B> {
    First(A),
    Second(B),
}
impl<A, B> Switched<A, B> {
    pub const fn is_first(&self) -> bool {
        matches!(self, Self::First(_))
    }
    pub fn first(&self) -> Option<&A> {
        match self {
            Self::First(value) => Some(value),
            Self::Second(_) => None,
        }
    }
    pub fn second(&self) -> Option<&B> {
        match self {
            Self::First(_) => None,
            Self::Second(value) => Some(value),
        }
    }
    pub fn first_mut(&mut self) -> Option<&mut A> {
        match self {
            Self::First(value) => Some(value),
            Self::Second(_) => None,
        }
    }
    pub fn second_mut(&mut self) -> Option<&mut B> {
        match self {
            Self::First(_) => None,
            Self::Second(value) => Some(value),
        }
    }
}
/// Original side diagnostics are forwarded unchanged.
impl<A: std::fmt::Display, B: std::fmt::Display> std::fmt::Display for Switched<A, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::First(value) => value.fmt(f),
            Self::Second(value) => value.fmt(f),
        }
    }
}
impl<A, B> std::error::Error for Switched<A, B>
where
    A: std::error::Error + 'static,
    B: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::First(error) => Some(error),
            Self::Second(error) => Some(error),
        }
    }
}
impl<A: StoppedMixerSource, B: StoppedMixerSource> StoppedMixerSource for Switched<A, B> {
    type Error = Switched<A::Error, B::Error>;
    fn take_stopped_mixer(&mut self) -> Result<Option<Mixer>, Self::Error> {
        match self {
            Self::First(output) => output.take_stopped_mixer().map_err(Switched::First),
            Self::Second(output) => output.take_stopped_mixer().map_err(Switched::Second),
        }
    }
}

/// Both statically injected platform adapters stay owned; only outputs alternate.
pub struct OutputBackendSwitch<A, B> {
    first: A,
    second: B,
}
impl<A, B> OutputBackendSwitch<A, B> {
    pub const fn new(first: A, second: B) -> Self {
        Self { first, second }
    }
    pub fn first(&self) -> &A {
        &self.first
    }
    pub fn second(&self) -> &B {
        &self.second
    }
    pub fn first_mut(&mut self) -> &mut A {
        &mut self.first
    }
    pub fn second_mut(&mut self) -> &mut B {
        &mut self.second
    }
    pub fn into_parts(self) -> (A, B) {
        (self.first, self.second)
    }
}

/// Retags a side failure without dropping its mixer, pending owner or cleanup.
fn switched_failure<E, S, F, T>(
    failure: OutputOpenFailure<E, S>,
    error: impl Fn(E) -> F,
    owner: impl FnOnce(S) -> T,
) -> OutputOpenFailure<F, T> {
    let (original, mixer, pending, cleanup) = failure.into_parts();
    // OutputOpenFailure constructors never retain both a pending owner and a mixer.
    debug_assert!(mixer.is_none() || pending.is_none());
    let failure = match pending {
        Some(pending) => OutputOpenFailure::pending(error(original), owner(pending)),
        None => OutputOpenFailure::recovered(error(original), mixer),
    };
    match cleanup {
        Some(cleanup) => failure.with_cleanup_error(error(cleanup)),
        None => failure,
    }
}

impl<A, B> OutputReplacementBackend for OutputBackendSwitch<A, B>
where
    A: OutputReplacementBackend,
    B: OutputReplacementBackend<Presentation = A::Presentation>,
{
    type Presentation = A::Presentation;
    type Output = Switched<A::Output, B::Output>;
    type Request = Switched<A::Request, B::Request>;
    type Error = Switched<A::Error, B::Error>;
    fn open(
        &mut self,
        request: Self::Request,
        mixer: Mixer,
        epoch: u64,
    ) -> Result<Self::Output, OutputOpenFailure<Self::Error, Self::Output>> {
        match request {
            Switched::First(request) => self
                .first
                .open(request, mixer, epoch)
                .map(Switched::First)
                .map_err(|failure| switched_failure(failure, Switched::First, Switched::First)),
            Switched::Second(request) => self
                .second
                .open(request, mixer, epoch)
                .map(Switched::Second)
                .map_err(|failure| switched_failure(failure, Switched::Second, Switched::Second)),
        }
    }
    fn retire(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        match output {
            Switched::First(output) => self.first.retire(output).map_err(Switched::First),
            Switched::Second(output) => self.second.retire(output).map_err(Switched::Second),
        }
    }
    fn start(&mut self, output: &mut Self::Output) -> Result<(), Self::Error> {
        match output {
            Switched::First(output) => self.first.start(output).map_err(Switched::First),
            Switched::Second(output) => self.second.start(output).map_err(Switched::Second),
        }
    }
    fn epoch(&self, output: &Self::Output) -> u64 {
        match output {
            Switched::First(output) => self.first.epoch(output),
            Switched::Second(output) => self.second.epoch(output),
        }
    }
    fn basis(&self, output: &Self::Output) -> OutputFrameBasis {
        match output {
            Switched::First(output) => self.first.basis(output),
            Switched::Second(output) => self.second.basis(output),
        }
    }
    fn observe(
        &mut self,
        output: &mut Self::Output,
        presentation: &mut Self::Presentation,
    ) -> Result<(), Self::Error> {
        match output {
            Switched::First(output) => self
                .first
                .observe(output, presentation)
                .map_err(Switched::First),
            Switched::Second(output) => self
                .second
                .observe(output, presentation)
                .map_err(Switched::Second),
        }
    }
    /// Forward overrides so interval backends keep their original end evidence.
    fn observe_end(
        &self,
        output: &Self::Output,
        end: &mut NativeEnd,
        pair: ClockPair,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        match output {
            Switched::First(output) => self.first.observe_end(output, end, pair, report),
            Switched::Second(output) => self.second.observe_end(output, end, pair, report),
        }
    }
    fn render_report(&self, output: &Self::Output) -> Result<Option<RenderReport>, Self::Error> {
        match output {
            Switched::First(output) => self.first.render_report(output).map_err(Switched::First),
            Switched::Second(output) => self.second.render_report(output).map_err(Switched::Second),
        }
    }
    fn pause_observation(
        &self,
        output: &Self::Output,
        pair: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, Self::Error> {
        match output {
            Switched::First(output) => self
                .first
                .pause_observation(output, pair, now)
                .map_err(Switched::First),
            Switched::Second(output) => self
                .second
                .pause_observation(output, pair, now)
                .map_err(Switched::Second),
        }
    }
}
#[cfg(test)]
#[path = "switch_fixtures.rs"]
mod fixtures;
