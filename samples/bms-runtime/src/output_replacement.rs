//! Cold output replacement policy; native lifecycle and observations are explicit ports.
use crate::{
    gameplay_presentation::{
        GameplayPresentationPort, PreparedOutputTiming, prepare_output_timing_rebind,
    },
    live_pause::LivePauseObservation,
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::{
        Mixer, OutputFrameBasis, OutputOpenFailure, PauseHold, PauseHoldError, RenderReport,
        StoppedMixerSource,
    },
    time::{ClockPair, ClockPoint, Timestamp},
};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementState {
    Detached,
    Attached,
    RecoveredMixer,
    PendingRetirement,
    Waiting,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplacementPhase {
    Retire,
    Recover,
    Open,
    Start,
    Observe,
}
pub enum ReplacementCause<E> {
    Policy(&'static str),
    Timing(Box<dyn std::error::Error>),
    Hold(PauseHoldError),
    Backend { phase: ReplacementPhase, error: E },
}
pub struct ReplacementFailure<E> {
    pub cause: ReplacementCause<E>,
    pub cleanup: Option<E>,
    pub recovery: Option<E>,
}
impl<E> ReplacementFailure<E> {
    fn policy(message: &'static str) -> Self {
        Self {
            cause: ReplacementCause::Policy(message),
            cleanup: None,
            recovery: None,
        }
    }
    fn timing(error: Box<dyn std::error::Error>) -> Self {
        Self {
            cause: ReplacementCause::Timing(error),
            cleanup: None,
            recovery: None,
        }
    }
    fn backend(phase: ReplacementPhase, error: E) -> Self {
        Self {
            cause: ReplacementCause::Backend { phase, error },
            cleanup: None,
            recovery: None,
        }
    }
}
pub struct ReadyOutput<P: GameplayPresentationPort, O> {
    pub output: O,
    pub timing: PreparedOutputTiming<P>,
    pub hold: PauseHold,
}
struct Waiting<B: OutputReplacementBackend> {
    output: B::Output,
    timing: PreparedOutputTiming<B::Presentation>,
    hold: PauseHold,
    wait_ns: u64,
    first_poll: Option<ClockPoint>,
    last_poll: Option<ClockPoint>,
}
enum Slot<B: OutputReplacementBackend> {
    Detached,
    Attached(B::Output),
    Recovered(Mixer),
    Pending(B::Output),
    Waiting(Waiting<B>),
    Unavailable,
}
pub struct OutputReplacement<B: OutputReplacementBackend> {
    backend: B,
    slot: Slot<B>,
    last_issued: u64,
}
impl<B: OutputReplacementBackend> OutputReplacement<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            slot: Slot::Detached,
            last_issued: 0,
        }
    }
    pub fn state(&self) -> ReplacementState {
        match &self.slot {
            Slot::Detached => ReplacementState::Detached,
            Slot::Attached(_) => ReplacementState::Attached,
            Slot::Recovered(_) => ReplacementState::RecoveredMixer,
            Slot::Pending(_) => ReplacementState::PendingRetirement,
            Slot::Waiting(_) => ReplacementState::Waiting,
            Slot::Unavailable => ReplacementState::Unavailable,
        }
    }
    pub const fn last_issued_epoch(&self) -> u64 {
        self.last_issued
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }
    /// Rejected ownership is returned untouched, never implicitly dropped.
    pub fn attach(&mut self, output: B::Output) -> Result<(), B::Output> {
        if !matches!(&self.slot, Slot::Detached | Slot::Unavailable) {
            return Err(output);
        }
        self.last_issued = self.last_issued.max(self.backend.epoch(&output));
        self.slot = Slot::Attached(output);
        Ok(())
    }
    pub fn take_recovered_mixer(&mut self) -> Option<Mixer> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        match slot {
            Slot::Recovered(mixer) => Some(mixer),
            other => {
                self.slot = other;
                None
            }
        }
    }
    fn cleanup_output(&mut self, mut output: B::Output) -> (Option<B::Error>, Option<B::Error>) {
        let cleanup = self.backend.retire(&mut output).err();
        match output.take_stopped_mixer() {
            Ok(Some(mixer)) => {
                self.slot = Slot::Recovered(mixer);
                (cleanup, None)
            }
            Ok(None) => {
                self.slot = Slot::Unavailable;
                (cleanup, None)
            }
            Err(error) => {
                self.slot = Slot::Pending(output);
                (cleanup, Some(error))
            }
        }
    }
    pub fn begin(
        &mut self,
        request: B::Request,
        current: &B::Presentation,
        pause: &NativePause,
        original_song: Timestamp,
        wait_ns: u64,
        acquire: impl FnOnce() -> Result<PauseHold, PauseHoldError>,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if pause.phase() != PausePhase::Paused {
            return Err(ReplacementFailure::policy(
                "output replacement requires acknowledged pause",
            ));
        }
        let epoch = current.epoch().ok_or_else(|| {
            ReplacementFailure::policy("output replacement requires presentation epochs")
        })?;
        if epoch != pause.epoch() {
            return Err(ReplacementFailure::policy(
                "pause and presentation output epochs differ",
            ));
        }
        if wait_ns == 0 {
            return Err(ReplacementFailure::policy(
                "output replacement wait must be positive",
            ));
        }
        match &self.slot {
            Slot::Attached(output) if self.backend.epoch(output) == epoch => {}
            Slot::Recovered(_) => {}
            Slot::Attached(_) => {
                return Err(ReplacementFailure::policy(
                    "attached output epoch differs from paused clocks",
                ));
            }
            _ => {
                return Err(ReplacementFailure::policy(
                    "output replacement ownership is unavailable or busy",
                ));
            }
        }
        let next_epoch = self
            .last_issued
            .max(epoch)
            .checked_add(1)
            .ok_or_else(|| ReplacementFailure::policy("output replacement epoch exhausted"))?;
        let hold = acquire().map_err(|error| ReplacementFailure {
            cause: ReplacementCause::Hold(error),
            cleanup: None,
            recovery: None,
        })?;
        let mixer = match std::mem::replace(&mut self.slot, Slot::Detached) {
            Slot::Recovered(mixer) => mixer,
            Slot::Attached(mut output) => {
                let retirement = self.backend.retire(&mut output);
                let recovered = output.take_stopped_mixer();
                match (retirement, recovered) {
                    (Ok(()), Ok(Some(mixer))) => mixer,
                    (Err(error), Ok(Some(mixer))) => {
                        self.slot = Slot::Recovered(mixer);
                        return Err(ReplacementFailure::backend(ReplacementPhase::Retire, error));
                    }
                    (Err(error), Ok(None)) => {
                        self.slot = Slot::Unavailable;
                        return Err(ReplacementFailure::backend(ReplacementPhase::Retire, error));
                    }
                    (Ok(()), Ok(None)) => {
                        self.slot = Slot::Unavailable;
                        return Err(ReplacementFailure::policy(
                            "retired output has no recoverable mixer",
                        ));
                    }
                    (Err(error), Err(recovery)) => {
                        self.slot = Slot::Pending(output);
                        let mut failure =
                            ReplacementFailure::backend(ReplacementPhase::Retire, error);
                        failure.recovery = Some(recovery);
                        return Err(failure);
                    }
                    (Ok(()), Err(error)) => {
                        self.slot = Slot::Pending(output);
                        return Err(ReplacementFailure::backend(
                            ReplacementPhase::Recover,
                            error,
                        ));
                    }
                }
            }
            _ => unreachable!("preflight admitted only attached or recovered ownership"),
        };
        let timing =
            match prepare_output_timing_rebind(current, pause, next_epoch, &mixer, original_song) {
                Ok(timing) => timing,
                Err(error) => {
                    self.slot = Slot::Recovered(mixer);
                    return Err(ReplacementFailure::timing(error));
                }
            };
        // Consume creation identity before every real attempt, including refusal.
        self.last_issued = next_epoch;
        let mut output = match self.backend.open(request, mixer, next_epoch) {
            Ok(output) => output,
            Err(failure) => {
                let (error, mixer, pending, cleanup) = failure.into_parts();
                self.slot = match (pending, mixer) {
                    (Some(output), _) => Slot::Pending(output),
                    (None, Some(mixer)) => Slot::Recovered(mixer),
                    (None, None) => Slot::Unavailable,
                };
                return Err(ReplacementFailure {
                    cause: ReplacementCause::Backend {
                        phase: ReplacementPhase::Open,
                        error,
                    },
                    cleanup,
                    recovery: None,
                });
            }
        };
        if self.backend.epoch(&output) != next_epoch || self.backend.basis(&output) != timing.basis
        {
            let (cleanup, recovery) = self.cleanup_output(output);
            return Err(ReplacementFailure {
                cause: ReplacementCause::Policy("opened output epoch or frame basis differs"),
                cleanup,
                recovery,
            });
        }
        if let Err(error) = self.backend.start(&mut output) {
            let (cleanup, recovery) = self.cleanup_output(output);
            return Err(ReplacementFailure {
                cause: ReplacementCause::Backend {
                    phase: ReplacementPhase::Start,
                    error,
                },
                cleanup,
                recovery,
            });
        }
        self.slot = Slot::Waiting(Waiting {
            output,
            timing,
            hold,
            wait_ns,
            first_poll: None,
            last_poll: None,
        });
        Ok(())
    }
    pub fn poll(
        &mut self,
        now: ClockPoint,
    ) -> Result<Option<ReadyOutput<B::Presentation, B::Output>>, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Waiting(mut waiting) = slot else {
            self.slot = slot;
            return Err(ReplacementFailure::policy(
                "output replacement is not waiting",
            ));
        };
        let result = (|| -> Result<bool, ReplacementFailure<B::Error>> {
            if now.domain != waiting.timing.pause.host_domain()
                || waiting
                    .last_poll
                    .is_some_and(|last| now.timestamp < last.timestamp)
            {
                return Err(ReplacementFailure::policy(
                    "output replacement poll clock changed or regressed",
                ));
            }
            let first = waiting.first_poll.get_or_insert(now);
            let elapsed =
                i128::from(now.timestamp.as_nanos()) - i128::from(first.timestamp.as_nanos());
            if elapsed >= i128::from(waiting.wait_ns) {
                return Err(ReplacementFailure::policy(
                    "output replacement observation timed out",
                ));
            }
            waiting.last_poll = Some(now);
            if self.backend.epoch(&waiting.output) != waiting.timing.pause.epoch()
                || self.backend.basis(&waiting.output) != waiting.timing.basis
            {
                return Err(ReplacementFailure::policy(
                    "waiting output epoch or frame basis changed",
                ));
            }
            self.backend
                .observe(&mut waiting.output, &mut waiting.timing.presentation)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?;
            if waiting.timing.presentation.epoch() != Some(waiting.timing.pause.epoch()) {
                return Err(ReplacementFailure::policy(
                    "replacement presentation epoch changed",
                ));
            }
            let Some(pair) = waiting.timing.presentation.latest_pair() else {
                return Ok(false);
            };
            waiting
                .timing
                .presentation
                .validate_host(now)
                .map_err(ReplacementFailure::timing)?;
            let Some(report) = self
                .backend
                .render_report(&waiting.output)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?
            else {
                return Ok(false);
            };
            if !report.paused {
                return Err(ReplacementFailure::policy(
                    "replacement output rendered without held pause",
                ));
            }
            if report.frames == 0 {
                return Ok(false);
            }
            let evidence = self
                .backend
                .pause_observation(&waiting.output, pair, now)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?;
            let epoch = waiting.timing.pause.epoch();
            match evidence {
                LivePauseObservation::Point(pair) => {
                    waiting
                        .timing
                        .pause
                        .observe_in_epoch(epoch, Some(report), pair)
                        .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
                }
                LivePauseObservation::Interval { observation, now } => {
                    let Some(observation) = observation else {
                        return Ok(false);
                    };
                    if observation.render != report {
                        return Err(ReplacementFailure::policy(
                            "replacement interval differs from actual render report",
                        ));
                    }
                    waiting
                        .timing
                        .pause
                        .observe_interval_in_epoch(epoch, Some(observation), now)
                        .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
                }
            }
            if waiting.timing.pause.phase() != PausePhase::Paused {
                return Err(ReplacementFailure::policy(
                    "replacement changed frozen pause state",
                ));
            }
            Ok(true)
        })();
        match result {
            Ok(false) => {
                self.slot = Slot::Waiting(waiting);
                Ok(None)
            }
            Ok(true) => Ok(Some(ReadyOutput {
                output: waiting.output,
                timing: waiting.timing,
                hold: waiting.hold,
            })),
            Err(mut failure) => {
                let (cleanup, recovery) = self.cleanup_output(waiting.output);
                failure.cleanup = cleanup;
                failure.recovery = recovery;
                Err(failure)
            }
        }
    }
    pub fn cancel(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Waiting(waiting) = slot else {
            self.slot = slot;
            return Ok(false);
        };
        let (cleanup, recovery) = self.cleanup_output(waiting.output);
        match (cleanup, recovery) {
            (Some(error), recovery) => {
                let mut failure = ReplacementFailure::backend(ReplacementPhase::Retire, error);
                failure.recovery = recovery;
                Err(failure)
            }
            (None, Some(error)) => Err(ReplacementFailure::backend(
                ReplacementPhase::Recover,
                error,
            )),
            (None, None) => Ok(true),
        }
    }
    pub fn retry_retirement(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Pending(output) = slot else {
            self.slot = slot;
            return Ok(false);
        };
        let (cleanup, recovery) = self.cleanup_output(output);
        match (cleanup, recovery) {
            (Some(error), recovery) => {
                let mut failure = ReplacementFailure::backend(ReplacementPhase::Retire, error);
                failure.recovery = recovery;
                Err(failure)
            }
            (None, Some(error)) => Err(ReplacementFailure::backend(
                ReplacementPhase::Recover,
                error,
            )),
            (None, None) => Ok(true),
        }
    }
}
#[cfg(test)]
#[path = "output_replacement_fixtures.rs"]
mod fixtures;
