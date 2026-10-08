//! Cold output replacement policy; native lifecycle and observations are explicit ports.
use crate::{
    audio_authority::AudioAuthorityEpoch,
    gameplay_presentation::{
        prepare_output_timing_rebind_state, GameplayPresentationPort, PreparedOutputTiming,
    },
    live_pause::LivePauseObservation,
    local_input::InputMerger,
    native_audio_presentation::{
        prepare_native_snapshot, NativeAudioPresentation, NativeAudioSnapshot,
        PreparedNativeAudioEpoch,
    },
    playback_pause::{NativePause, PausePhase},
};
use beatkernel::{
    audio::{
        Mixer, OutputFrameBasis, PauseHold, PauseHoldError, SoftwareOutputState, StoppedMixerSource,
    },
    time::{ClockPoint, Timestamp},
};

use crate::gameplay::output::ports::{OriginalNativeOutputBackend, OutputReplacementBackend};
#[cfg(test)]
use beatkernel::audio::{OutputOpenFailure, RenderReport};
use beatkernel_platform::audio::presentation::validation::{
    NativePresentationValidator, OriginalNativePresentationEvidence,
};
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
    pub(super) fn policy(message: &'static str) -> Self {
        Self {
            cause: ReplacementCause::Policy(message),
            cleanup: None,
            recovery: None,
        }
    }
    pub(super) fn timing(error: Box<dyn std::error::Error>) -> Self {
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
pub struct ReadyAudioOutput<O> {
    pub output: O,
    pub pause: NativePause,
    pub basis: OutputFrameBasis,
    pub playback_origin: ClockPoint,
    pub snapshots: [NativeAudioSnapshot; 2],
    pub prepared: PreparedNativeAudioEpoch,
    pub hold: PauseHold,
}
struct AudioWaitingTiming {
    pause: NativePause,
    basis: OutputFrameBasis,
    playback_origin: ClockPoint,
    next: AudioAuthorityEpoch,
    validator: NativePresentationValidator,
    snapshots: [Option<NativeAudioSnapshot>; 2],
    pairs: [Option<beatkernel::time::ClockPair>; 2],
}
#[path = "target_replacement.rs"]
pub mod target;
enum WaitingTiming<P: GameplayPresentationPort> {
    Legacy(PreparedOutputTiming<P>),
    Audio(AudioWaitingTiming),
    Target(target::TargetWaitingTiming),
}
impl<P: GameplayPresentationPort> WaitingTiming<P> {
    fn basis(&self) -> Result<OutputFrameBasis, &'static str> {
        match self {
            Self::Legacy(value) => Ok(value.basis),
            Self::Audio(value) => Ok(value.basis),
            Self::Target(_) => Err("target timing has no fixed source-grid basis"),
        }
    }
}
struct Waiting<B: OutputReplacementBackend<O, Basis>, O = Mixer, Basis = OutputFrameBasis> {
    output: B::Output,
    timing: WaitingTiming<B::Presentation>,
    hold: PauseHold,
    wait_ns: u64,
    first_poll: Option<ClockPoint>,
    last_poll: Option<ClockPoint>,
}
enum Slot<B: OutputReplacementBackend<O, Basis>, O = Mixer, Basis = OutputFrameBasis> {
    Detached,
    Attached(B::Output),
    Recovered(O),
    Pending(B::Output),
    Waiting(Waiting<B, O, Basis>),
    Unavailable,
}
pub struct OutputReplacement<
    B: OutputReplacementBackend<O, Basis>,
    O = Mixer,
    Basis = OutputFrameBasis,
> {
    backend: B,
    slot: Slot<B, O, Basis>,
    last_issued: u64,
    retirement_hold: Option<PauseHold>,
}
impl<B: OutputReplacementBackend<O, Basis>, O, Basis> OutputReplacement<B, O, Basis> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            slot: Slot::Detached,
            last_issued: 0,
            retirement_hold: None,
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
        if self.state() != ReplacementState::PendingRetirement {
            self.retirement_hold = None;
        }
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
    pub(super) fn retain_retirement_hold(&mut self, hold: PauseHold) {
        if self.state() == ReplacementState::PendingRetirement {
            self.retirement_hold = Some(hold);
        }
    }
    pub(super) fn retire_owned_output_held(
        &mut self,
        output: B::Output,
        hold: PauseHold,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        let result = self.retire_owned_output(output);
        self.retain_retirement_hold(hold);
        result
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
    pub fn take_recovered_mixer(&mut self) -> Option<O> {
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
}
impl<B: OutputReplacementBackend<O, Basis>, O, Basis: PartialEq> OutputReplacement<B, O, Basis> {
    fn begin_lifecycle_with(
        &mut self,
        request: B::Request,
        epoch: u64,
        pause: &NativePause,
        wait_ns: u64,
        acquire: impl FnOnce() -> Result<PauseHold, PauseHoldError>,
        prepare: impl FnOnce(
            &B,
            &B::Request,
            u64,
            &O,
        ) -> Result<
            (WaitingTiming<B::Presentation>, Basis),
            ReplacementFailure<B::Error>,
        >,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if pause.phase() != PausePhase::Paused {
            return Err(ReplacementFailure::policy(
                "output replacement requires acknowledged pause",
            ));
        }
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
                        self.retain_retirement_hold(hold);
                        let mut failure =
                            ReplacementFailure::backend(ReplacementPhase::Retire, error);
                        failure.recovery = Some(recovery);
                        return Err(failure);
                    }
                    (Ok(()), Err(error)) => {
                        self.slot = Slot::Pending(output);
                        self.retain_retirement_hold(hold);
                        return Err(ReplacementFailure::backend(
                            ReplacementPhase::Recover,
                            error,
                        ));
                    }
                }
            }
            _ => unreachable!("preflight admitted only attached or recovered ownership"),
        };
        let (timing, basis) = match prepare(&self.backend, &request, next_epoch, &mixer) {
            Ok(timing) => timing,
            Err(error) => {
                self.slot = Slot::Recovered(mixer);
                return Err(error);
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
                self.retain_retirement_hold(hold);
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
        if self.backend.epoch(&output) != next_epoch || self.backend.basis(&output) != basis {
            let (cleanup, recovery) = self.cleanup_output(output);
            self.retain_retirement_hold(hold);
            return Err(ReplacementFailure {
                cause: ReplacementCause::Policy("opened output epoch or frame basis differs"),
                cleanup,
                recovery,
            });
        }
        if let Err(error) = self
            .backend
            .prepare_replacement_start(&mut output)
            .and_then(|_| self.backend.start(&mut output))
        {
            let (cleanup, recovery) = self.cleanup_output(output);
            self.retain_retirement_hold(hold);
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
}
impl<B: OutputReplacementBackend<O>, O: SoftwareOutputState> OutputReplacement<B, O> {
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
        self.begin_with(request, epoch, pause, wait_ns, acquire, |next, mixer| {
            prepare_output_timing_rebind_state(current, pause, next, mixer, original_song)
                .map(WaitingTiming::Legacy)
        })
    }
    fn begin_with(
        &mut self,
        request: B::Request,
        epoch: u64,
        pause: &NativePause,
        wait_ns: u64,
        acquire: impl FnOnce() -> Result<PauseHold, PauseHoldError>,
        prepare: impl FnOnce(
            u64,
            &O,
        )
            -> Result<WaitingTiming<B::Presentation>, Box<dyn std::error::Error>>,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        self.begin_lifecycle_with(
            request,
            epoch,
            pause,
            wait_ns,
            acquire,
            |_, _, next, owner| {
                if !owner.paused_tail_admissible() {
                    return Err(ReplacementFailure::policy(
                        "held output replacement requires a proven paused-zero retained tail",
                    ));
                }
                let timing = prepare(next, owner).map_err(ReplacementFailure::timing)?;
                let basis = timing.basis().map_err(ReplacementFailure::policy)?;
                Ok((timing, basis))
            },
        )
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
        let WaitingTiming::Legacy(timing) = &mut waiting.timing else {
            self.slot = Slot::Waiting(waiting);
            return Err(ReplacementFailure::policy(
                "legacy polling cannot consume audio timing",
            ));
        };
        let result = (|| -> Result<bool, ReplacementFailure<B::Error>> {
            if now.domain != timing.pause.host_domain()
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
            if self.backend.epoch(&waiting.output) != timing.pause.epoch()
                || self.backend.basis(&waiting.output) != timing.basis
            {
                return Err(ReplacementFailure::policy(
                    "waiting output epoch or frame basis changed",
                ));
            }
            self.backend
                .observe_replacement(&mut waiting.output, &mut timing.presentation, &timing.pause)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?;
            if timing.presentation.epoch() != Some(timing.pause.epoch()) {
                return Err(ReplacementFailure::policy(
                    "replacement presentation epoch changed",
                ));
            }
            let Some(pair) = timing.presentation.latest_pair() else {
                return Ok(false);
            };
            timing
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
            if !timing
                .pause
                .replacement_observation_ready(Some(report), pair)
            {
                return Ok(false);
            }
            let evidence = self
                .backend
                .pause_observation(&waiting.output, pair, now)
                .map_err(|error| ReplacementFailure::backend(ReplacementPhase::Observe, error))?;
            let epoch = timing.pause.epoch();
            match evidence {
                LivePauseObservation::Target { .. } => {
                    return Err(ReplacementFailure::policy(
                        "source-grid replacement cannot consume target pause evidence",
                    ));
                }
                LivePauseObservation::Point(pair) => {
                    timing
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
                    timing
                        .pause
                        .observe_interval_in_epoch(epoch, Some(observation), now)
                        .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
                }
            }
            if timing.pause.phase() != PausePhase::Paused {
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
            Ok(true) => {
                let WaitingTiming::Legacy(timing) = waiting.timing else {
                    unreachable!("legacy timing checked before polling")
                };
                Ok(Some(ReadyOutput {
                    output: waiting.output,
                    timing,
                    hold: waiting.hold,
                }))
            }
            Err(mut failure) => {
                let (cleanup, recovery) = self.cleanup_output(waiting.output);
                self.retain_retirement_hold(waiting.hold);
                failure.cleanup = cleanup;
                failure.recovery = recovery;
                Err(failure)
            }
        }
    }
}
impl<B: OutputReplacementBackend<O, Basis>, O, Basis> OutputReplacement<B, O, Basis> {
    pub fn cancel(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Waiting(waiting) = slot else {
            self.slot = slot;
            return Ok(false);
        };
        let (cleanup, recovery) = self.cleanup_output(waiting.output);
        self.retain_retirement_hold(waiting.hold);
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
        if self.state() != ReplacementState::PendingRetirement {
            self.retirement_hold = None;
        }
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
impl<B: OriginalNativeOutputBackend<O>, O: SoftwareOutputState> OutputReplacement<B, O> {
    pub fn begin_audio(
        &mut self,
        request: B::Request,
        current: &NativeAudioPresentation,
        pause: &NativePause,
        merger: &InputMerger,
        original_song: Timestamp,
        wait_ns: u64,
        acquire: impl FnOnce() -> Result<PauseHold, PauseHoldError>,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if merger.pending() != 0 {
            return Err(ReplacementFailure::policy(
                "audio replacement requires drained paused input",
            ));
        }
        let epoch = current.authority().epoch();
        self.begin_with(request, epoch.id, pause, wait_ns, acquire, |id, mixer| {
            if !mixer.mixer().pause_requested() {
                return Err("audio replacement requires a held producer pause".into());
            }
            let basis = mixer.output_frame_basis();
            let playback_origin = basis.point_at_stream_frame(0)?;
            let mut candidate_pause = pause.clone();
            candidate_pause.rebind_output_state(id, mixer)?;
            candidate_pause.song_origin_for_presentation(original_song, playback_origin)?;
            let next = AudioAuthorityEpoch {
                id,
                stream_origin: playback_origin,
                logical_origin: current
                    .authority()
                    .checked_logical_output(playback_origin)?,
                host_domain: epoch.host_domain,
            };
            Ok(WaitingTiming::Audio(AudioWaitingTiming {
                pause: candidate_pause,
                basis,
                playback_origin,
                next,
                validator: NativePresentationValidator::new(id, playback_origin, epoch.host_domain),
                snapshots: [None; 2],
                pairs: [None; 2],
            }))
        })
    }

    pub fn poll_audio(
        &mut self,
        current: &NativeAudioPresentation,
        merger: &InputMerger,
        now: ClockPoint,
    ) -> Result<Option<ReadyAudioOutput<B::Output>>, ReplacementFailure<B::Error>> {
        let slot = std::mem::replace(&mut self.slot, Slot::Detached);
        let Slot::Waiting(mut waiting) = slot else {
            self.slot = slot;
            return Err(ReplacementFailure::policy(
                "output replacement is not waiting",
            ));
        };
        let WaitingTiming::Audio(timing) = &mut waiting.timing else {
            self.slot = Slot::Waiting(waiting);
            return Err(ReplacementFailure::policy(
                "audio polling cannot consume legacy timing",
            ));
        };
        let result =
            (|| -> Result<Option<PreparedNativeAudioEpoch>, ReplacementFailure<B::Error>> {
                if now.domain != timing.pause.host_domain()
                    || waiting
                        .last_poll
                        .is_some_and(|last| now.timestamp < last.timestamp)
                {
                    return Err(ReplacementFailure::policy(
                        "output replacement poll clock changed or regressed",
                    ));
                }
                let first = waiting.first_poll.get_or_insert(now);
                if i128::from(now.timestamp.as_nanos()) - i128::from(first.timestamp.as_nanos())
                    >= i128::from(waiting.wait_ns)
                {
                    return Err(ReplacementFailure::policy(
                        "output replacement observation timed out",
                    ));
                }
                waiting.last_poll = Some(now);
                if self.backend.epoch(&waiting.output) != timing.next.id
                    || self.backend.basis(&waiting.output) != timing.basis
                {
                    return Err(ReplacementFailure::policy(
                        "waiting output epoch or frame basis changed",
                    ));
                }
                // Once two real anchors are selected, retain them while their
                // original future association/bracket becomes covered. Newer
                // latency-shifted readings must not move that target forever.
                let refresh = if let Some(second) = timing.pairs[1] {
                    let upper = match timing.snapshots[1]
                        .expect("second pair retains snapshot")
                        .evidence
                    {
                        OriginalNativePresentationEvidence::Asio { observation, .. } => {
                            observation.host.after
                        }
                        _ => second.target,
                    };
                    let covered = upper.timestamp <= now.timestamp
                        && second.target.timestamp <= now.timestamp;
                    let host_incompatible = [
                        current.authority().closed_host_prefix(),
                        current.authority().committed_input_host(),
                        current
                            .authority()
                            .latest_observation()
                            .map(|pair| pair.target),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|last| second.target.timestamp < last.timestamp);
                    let first_output = current
                        .authority()
                        .checked_logical_output(timing.pairs[0].expect("first pair").source)
                        .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
                    covered
                        && (i128::from(now.timestamp.as_nanos())
                            - i128::from(second.target.timestamp.as_nanos())
                            > i128::from(
                                current.authority().config().max_observation_age.as_nanos(),
                            )
                            || host_incompatible
                            || current
                                .authority()
                                .committed_presentation()
                                .is_some_and(|last| first_output.timestamp < last.timestamp))
                } else {
                    false
                };
                let collect = timing.pairs[1].is_none() || refresh;
                let snapshot =
                    self.backend
                        .observe_native(&mut waiting.output)
                        .map_err(|error| {
                            ReplacementFailure::backend(ReplacementPhase::Observe, error)
                        })?;
                let backend_report =
                    self.backend
                        .render_report(&waiting.output)
                        .map_err(|error| {
                            ReplacementFailure::backend(ReplacementPhase::Observe, error)
                        })?;
                if let Some(snapshot) = snapshot {
                    if snapshot.epoch != timing.next.id || snapshot.basis != timing.basis {
                        return Err(ReplacementFailure::policy(
                            "candidate snapshot epoch or basis differs",
                        ));
                    }
                    let observed_report = match snapshot.evidence {
                        OriginalNativePresentationEvidence::Asio { observation, .. } => {
                            Some(observation.render)
                        }
                        OriginalNativePresentationEvidence::Wasapi { snapshot, .. } => {
                            snapshot.render.or(backend_report)
                        }
                        OriginalNativePresentationEvidence::SuppliedPair(_) => backend_report,
                    };
                    if observed_report.is_some_and(|report| !report.paused) {
                        return Err(ReplacementFailure::policy(
                            "replacement output rendered without held pause",
                        ));
                    }
                    // Preparation validates domains without committing evidence.
                    // Only valid early tail observations may wait for freshness.
                    let prepared = prepare_native_snapshot(&timing.validator, snapshot)
                        .map_err(ReplacementFailure::timing)?;
                    let pair = prepared.correlation_pair();
                    if pair.is_none_or(|pair| {
                        !timing
                            .pause
                            .replacement_observation_ready(observed_report, pair)
                    }) {
                        return Ok(None);
                    }
                    if collect {
                        timing
                            .validator
                            .commit(prepared)
                            .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
                        if let Some(pair) = pair {
                            let index = if timing.pairs[0].is_none() { 0 } else { 1 };
                            let previous = timing.pairs[1].or(timing.pairs[0]);
                            if previous.is_none_or(|old| {
                                pair.source.timestamp > old.source.timestamp
                                    && pair.target.timestamp > old.target.timestamp
                            }) {
                                if timing.pairs[1].is_some() {
                                    timing.pairs[0] = timing.pairs[1];
                                    timing.snapshots[0] = timing.snapshots[1];
                                }
                                timing.pairs[index] = Some(pair);
                                timing.snapshots[index] = Some(snapshot);
                            } else if previous.is_some_and(|old| {
                                pair.source.timestamp == old.source.timestamp
                                    && pair.target.timestamp > old.target.timestamp
                            }) {
                                // Native counter progress can quantize to the same raw
                                // nanosecond. Keep its newest original metadata without
                                // counting it as a second distinct raw anchor.
                                let latest = if timing.pairs[1].is_some() { 1 } else { 0 };
                                timing.pairs[latest] = Some(pair);
                                timing.snapshots[latest] = Some(snapshot);
                            }
                        }
                    }
                }
                if backend_report.is_some_and(|report| !report.paused) {
                    return Err(ReplacementFailure::policy(
                        "replacement output rendered without held pause",
                    ));
                }
                let Some(record) = timing.validator.latest_record() else {
                    return Ok(None);
                };
                let pair = record.pair();
                let report = match *record.evidence() {
                    OriginalNativePresentationEvidence::Asio { observation, .. } => {
                        Some(observation.render)
                    }
                    OriginalNativePresentationEvidence::Wasapi { snapshot, .. } => {
                        snapshot.render.or(backend_report)
                    }
                    OriginalNativePresentationEvidence::SuppliedPair(_) => backend_report,
                };
                let Some(report) = report else {
                    return Ok(None);
                };
                if !report.paused {
                    return Err(ReplacementFailure::policy(
                        "replacement output rendered without held pause",
                    ));
                }
                if report.frames == 0 || pair.target.timestamp > now.timestamp {
                    return Ok(None);
                }
                match *record.evidence() {
                    OriginalNativePresentationEvidence::Asio { observation, .. } => {
                        if observation.host.after.timestamp > now.timestamp {
                            return Ok(None);
                        }
                        let LivePauseObservation::Interval {
                            observation: Some(observation),
                            ..
                        } = crate::gameplay::output::adapters::observation::asio_pause_observation(
                            Some(observation),
                            now,
                        )
                        else {
                            unreachable!("original ASIO interval supplied")
                        };
                        timing
                            .pause
                            .observe_interval_in_epoch(timing.next.id, Some(observation), now)
                            .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
                    }
                    _ => {
                        timing
                            .pause
                            .observe_in_epoch(timing.next.id, Some(report), pair)
                            .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
                    }
                }
                if timing.pause.phase() != PausePhase::Paused {
                    return Err(ReplacementFailure::policy(
                        "replacement changed frozen pause state",
                    ));
                }
                let [Some(first), Some(second)] = timing.pairs else {
                    return Ok(None);
                };
                if merger.pending() != 0
                    || second.target.timestamp > now.timestamp
                    || i128::from(now.timestamp.as_nanos())
                        - i128::from(second.target.timestamp.as_nanos())
                        > i128::from(current.authority().config().max_observation_age.as_nanos())
                    || [
                        current.authority().closed_host_prefix(),
                        current.authority().committed_input_host(),
                        current
                            .authority()
                            .latest_observation()
                            .map(|pair| pair.target),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|last| second.target.timestamp < last.timestamp)
                {
                    return Ok(None);
                }
                let snapshots = [
                    timing.snapshots[0].expect("first pair retains snapshot"),
                    timing.snapshots[1].expect("second pair retains snapshot"),
                ];
                debug_assert!(first.source.timestamp < second.source.timestamp);
                current
                    .prepare_output_epoch(timing.next, timing.basis, snapshots, now, merger)
                    .map(Some)
                    .map_err(ReplacementFailure::timing)
            })();
        match result {
            Ok(None) => {
                self.slot = Slot::Waiting(waiting);
                Ok(None)
            }
            Ok(Some(prepared)) => {
                let WaitingTiming::Audio(timing) = waiting.timing else {
                    unreachable!("audio timing checked before polling")
                };
                Ok(Some(ReadyAudioOutput {
                    output: waiting.output,
                    pause: timing.pause,
                    basis: timing.basis,
                    playback_origin: timing.playback_origin,
                    snapshots: [
                        timing.snapshots[0].expect("first pair"),
                        timing.snapshots[1].expect("second pair"),
                    ],
                    prepared,
                    hold: waiting.hold,
                }))
            }
            Err(mut failure) => {
                let (cleanup, recovery) = self.cleanup_output(waiting.output);
                failure.cleanup = cleanup;
                self.retain_retirement_hold(waiting.hold);
                failure.recovery = recovery;
                Err(failure)
            }
        }
    }
}
#[cfg(test)]
#[path = "replacement_fixtures.rs"]
mod fixtures;

#[cfg(test)]
#[path = "target_lifecycle_fixtures.rs"]
mod target_lifecycle_fixtures;

#[cfg(test)]
#[path = "audio_publication_fixtures.rs"]
mod audio_publication_fixtures;

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

pub struct ReadyAudioPublicationFailure<O> {
    pub error: Box<dyn std::error::Error>,
    pub ready: ReadyAudioOutput<O>,
}

/// Stage all fallible checks before publishing authority or native ownership.
/// A refusal retains the original ready output and its exclusive producer hold.
pub fn publish_ready_audio_output<O>(
    ready: ReadyAudioOutput<O>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
) -> Result<(), ReadyAudioPublicationFailure<O>> {
    publish_ready_audio_output_held(ready, output, context, now).map(drop)
}

pub(crate) fn publish_ready_audio_output_held<O>(
    ready: ReadyAudioOutput<O>,
    output: &mut Option<O>,
    context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
    now: ClockPoint,
) -> Result<PauseHold, ReadyAudioPublicationFailure<O>> {
    let staged = (|| -> Result<_, Box<dyn std::error::Error>> {
        if output.is_some() {
            return Err("replacement output slot must be empty".into());
        }
        let old_epoch = context.presentation.authority().epoch();
        if old_epoch.id != context.pause.epoch()
            || ready.pause.epoch() != ready.prepared.epoch()
            || ready.pause.epoch() <= old_epoch.id
            || ready.prepared.basis() != ready.basis
            || ready.prepared.snapshots() != ready.snapshots
            || ready.basis.sample_rate() != context.config.sample_rate
            || ready.basis.origin().domain != context.config.playback_origin.domain
            || ready.pause.host_domain() != context.config.origin.domain
            || old_epoch.host_domain != context.config.origin.domain
            || ready.playback_origin != ready.basis.point_at_stream_frame(0)?
        {
            return Err("replacement audio timing/configuration identity differs".into());
        }
        context
            .presentation
            .validate_output_epoch(&ready.prepared, now, context.merger)?;
        let record = ready.prepared.latest_record();
        let pair = record.pair();
        context
            .pause
            .validate_replacement(&ready.pause, ready.basis, pair)?;
        let report = ready
            .pause
            .last_render_report()
            .ok_or("replacement lacks paused render history")?;
        let end = context
            .end
            .as_ref()
            .map(|end| match *record.evidence() {
                OriginalNativePresentationEvidence::Asio { observation, .. } => {
                    end.restart_for_output_asio(ready.basis, observation)
                }
                _ => end.restart_for_output(ready.basis, report, pair),
            })
            .transpose()?;
        Ok(end)
    })();
    let end = match staged {
        Ok(end) => end,
        Err(error) => return Err(ReadyAudioPublicationFailure { error, ready }),
    };
    // Exclusive synchronous commit cannot mutate native state between validation
    // and authority commit. No callback, IO or fallible stage follows success.
    if let Err(error) =
        context
            .presentation
            .commit_output_epoch(ready.prepared.clone(), now, context.merger)
    {
        return Err(ReadyAudioPublicationFailure { error, ready });
    }
    let ReadyAudioOutput {
        output: candidate,
        pause,
        playback_origin,
        hold,
        ..
    } = ready;
    *output = Some(candidate);
    *context.pause = pause;
    context.config.stream_origin = playback_origin;
    context.config.playback_origin = playback_origin;
    *context.end = end;
    Ok(hold)
}

#[cfg(test)]
#[path = "output_continuity_fixtures.rs"]
mod output_continuity_fixtures;
