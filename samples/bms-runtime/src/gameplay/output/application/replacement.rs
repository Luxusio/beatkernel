//! Cold output replacement policy; native lifecycle and observations are explicit ports.
use crate::{
    gameplay_presentation::{
        GameplayPresentationPort, PreparedOutputTiming, prepare_output_timing_rebind,
    },
    live_pause::LivePauseObservation,
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::{Mixer, PauseHold, PauseHoldError, StoppedMixerSource},
    time::{ClockPoint, Timestamp},
};

use crate::gameplay::output::ports::OutputReplacementBackend;
#[cfg(test)]
use beatkernel::audio::{OutputFrameBasis, OutputOpenFailure, RenderReport};
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
#[derive(Debug)]
pub enum ReplacementCause<E> {
    Policy(&'static str),
    Timing(Box<dyn std::error::Error>),
    Hold(PauseHoldError),
    Backend { phase: ReplacementPhase, error: E },
}
#[derive(Debug)]
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
impl<E: std::fmt::Display> std::fmt::Display for ReplacementFailure<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cause {
            ReplacementCause::Policy(message) => f.write_str(message)?,
            ReplacementCause::Timing(error) => error.fmt(f)?,
            ReplacementCause::Hold(error) => error.fmt(f)?,
            ReplacementCause::Backend { phase, error } => {
                write!(f, "output replacement {phase:?}: {error}")?
            }
        }
        if let Some(error) = &self.cleanup {
            write!(f, "; cleanup: {error}")?;
        }
        if let Some(error) = &self.recovery {
            write!(f, "; recovery: {error}")?;
        }
        Ok(())
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ReplacementFailure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            ReplacementCause::Timing(error) => Some(error.as_ref()),
            ReplacementCause::Hold(error) => Some(error),
            ReplacementCause::Backend { error, .. } => Some(error),
            _ => None,
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
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
    /// Explicitly retire owned rejected/other output through the same recovery policy.
    pub fn stop_owned(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        if self.state() == ReplacementState::Waiting {
            return self.cancel();
        }
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        match slot {
            Slot::Attached(output) | Slot::Pending(output) => {
                self.retire_owned_output(output).map(|_| true)
            }
            other => {
                self.slot = other;
                Ok(false)
            }
        }
    }
    pub fn retire_owned_output(
        &mut self,
        output: B::Output,
    ) -> Result<(), ReplacementFailure<B::Error>> {
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
            (None, None) => Ok(()),
        }
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
#[path = "replacement_fixtures.rs"]
mod fixtures;

/// Refusal returns the original ready owner and hold without publishing any target.
pub struct ReadyPublicationFailure<P: GameplayPresentationPort, O> {
    pub error: Box<dyn std::error::Error>,
    pub ready: ReadyOutput<P, O>,
}
/// Publishes all ownership only after every fallible timing/end check succeeds.
pub fn publish_ready_output<P: GameplayPresentationPort, O>(
    ready: ReadyOutput<P, O>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayOutputContext<'_, P>,
) -> Result<(), ReadyPublicationFailure<P, O>> {
    let staged = (|| -> Result<_, Box<dyn std::error::Error>> {
        if output.is_some() {
            return Err("replacement output slot must be empty".into());
        }
        let old_epoch = context
            .presentation
            .epoch()
            .ok_or("live presentation has no output epoch")?;
        if old_epoch != context.pause.epoch()
            || ready.timing.presentation.epoch() != Some(ready.timing.pause.epoch())
            || ready.timing.pause.epoch() <= old_epoch
            || ready.timing.presentation.config() != context.presentation.config()
            || ready.timing.basis.sample_rate() != context.config.sample_rate
            || ready.timing.basis.origin().domain != context.config.playback_origin.domain
            || ready.timing.pause.host_domain() != context.config.origin.domain
            || ready.timing.playback_origin != ready.timing.basis.point_at_stream_frame(0)?
        {
            return Err("replacement timing/configuration identity differs".into());
        }
        let pair = ready
            .timing
            .presentation
            .latest_pair()
            .ok_or("replacement lacks accepted presentation")?;
        ready.timing.presentation.validate_host(pair.target)?;
        context
            .pause
            .validate_replacement(&ready.timing.pause, ready.timing.basis, pair)?;
        if let Some(old) = context.presentation.latest_pair() {
            if pair.source.domain != old.source.domain
                || pair.target.domain != old.target.domain
                || pair.target.timestamp < old.target.timestamp
            {
                return Err("replacement presentation host history regressed".into());
            }
        }
        let report = ready
            .timing
            .pause
            .last_render_report()
            .ok_or("replacement lacks render history")?;
        let end = context
            .end
            .as_ref()
            .map(|end| end.restart_for_output(ready.timing.basis, report, pair))
            .transpose()?;
        Ok(end)
    })();
    let end = match staged {
        Ok(end) => end,
        Err(error) => return Err(ReadyPublicationFailure { error, ready }),
    };
    let ReadyOutput {
        output: candidate,
        timing,
        hold,
    } = ready;
    *output = Some(candidate);
    *context.presentation = timing.presentation;
    *context.pause = timing.pause;
    context.config.stream_origin = timing.playback_origin;
    context.config.playback_origin = timing.playback_origin;
    *context.end = end;
    drop(hold);
    Ok(())
}
