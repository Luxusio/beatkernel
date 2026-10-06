//! One static output owner shared by gameplay adapters; input and clocks stay external.
use crate::{
    gameplay_presentation::{GameplayOutputContext, GameplayPresentationPort},
    live_pause::LivePauseObservation,
    native_end::{NativeEnd, EndBoundary},
    output_replacement::{
        OutputReplacement, OutputReplacementBackend, ReplacementFailure, ReplacementCause,
        ReplacementPhase, ReplacementState, ReadyOutput, publish_ready_output,
    },
};
use beatkernel::{
    audio::{Mixer, RenderReport, OutputFrameBasis},
    time::{ClockPair, ClockPoint},
};

pub struct GameplayOutputOwner<B: OutputReplacementBackend> {
    current: Option<B::Output>,
    basis: OutputFrameBasis,
    controller: OutputReplacement<B>,
    pending: Option<(B::Request, u64)>,
    rejected: Option<ReadyOutput<B::Presentation, B::Output>>,
    report: Option<RenderReport>,
    pause_evidence: Option<LivePauseObservation>,
}
impl<B: OutputReplacementBackend> GameplayOutputOwner<B> {
    pub fn new(backend: B, output: B::Output) -> Self {
        let basis = backend.basis(&output);
        Self {
            basis,
            current: Some(output),
            controller: OutputReplacement::new(backend),
            pending: None,
            rejected: None,
            report: None,
            pause_evidence: None,
        }
    }
    pub fn current(&self) -> Option<&B::Output> {
        self.current.as_ref()
    }
    pub fn current_mut(&mut self) -> Option<&mut B::Output> {
        self.current.as_mut()
    }
    pub fn rejected_ready(&self) -> Option<&ReadyOutput<B::Presentation, B::Output>> {
        self.rejected.as_ref()
    }
    pub fn state(&self) -> ReplacementState {
        if self.current.is_some() {
            ReplacementState::Attached
        } else if self.rejected.is_some() {
            ReplacementState::Waiting
        } else {
            self.controller.state()
        }
    }
    pub fn last_issued_epoch(&self) -> u64 {
        self.current
            .as_ref()
            .map(|output| self.controller.backend().epoch(output))
            .unwrap_or(0)
            .max(self.controller.last_issued_epoch())
    }
    pub fn replacement_pending(&self) -> bool {
        self.rejected.is_some() || self.controller.state() == ReplacementState::Waiting
    }
    pub fn has_work(&self) -> bool {
        self.pending.is_some()
            || self.rejected.is_some()
            || self.controller.state() == ReplacementState::Waiting
    }
    pub fn queue(&mut self, request: B::Request, wait_ns: u64) -> Result<(), (B::Request, u64)> {
        if wait_ns == 0
            || self.pending.is_some()
            || self.rejected.is_some()
            || (self.current.is_none()
                && self.controller.state() != ReplacementState::RecoveredMixer)
        {
            return Err((request, wait_ns));
        }
        self.pending = Some((request, wait_ns));
        Ok(())
    }
    fn failure(phase: ReplacementPhase, error: B::Error) -> ReplacementFailure<B::Error> {
        ReplacementFailure {
            cause: ReplacementCause::Backend { phase, error },
            cleanup: None,
            recovery: None,
        }
    }
    pub fn observe(
        &mut self,
        presentation: &mut B::Presentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        let Some(output) = self.current.as_mut() else {
            return Ok(());
        };
        if presentation.epoch() != Some(self.controller.backend().epoch(output))
            || self.controller.backend().basis(output) != self.basis
        {
            return Err(ReplacementFailure {
                cause: ReplacementCause::Policy("current output epoch or frame basis differs"),
                cleanup: None,
                recovery: None,
            });
        }
        self.controller
            .backend_mut()
            .observe(output, presentation)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        if let Some(report) = self
            .controller
            .backend()
            .render_report(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?
        {
            self.report = Some(report);
        }
        Ok(())
    }
    pub fn render_report(&self) -> Option<RenderReport> {
        self.report
    }
    pub fn pause_observation(
        &mut self,
        pair: ClockPair,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, ReplacementFailure<B::Error>> {
        if let Some(output) = self.current.as_ref() {
            let evidence = self
                .controller
                .backend()
                .pause_observation(output, pair, now)
                .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
            self.pause_evidence = Some(evidence);
            Ok(evidence)
        } else {
            self.pause_evidence.ok_or(ReplacementFailure {
                cause: ReplacementCause::Policy("waiting output has no committed pause evidence"),
                cleanup: None,
                recovery: None,
            })
        }
    }
    pub fn seed_resume(
        &mut self,
        presentation: &mut B::Presentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if self.current.is_none() {
            return Err(ReplacementFailure {
                cause: ReplacementCause::Policy("resume requires a published output"),
                cleanup: None,
                recovery: None,
            });
        }
        self.observe(presentation)
    }
    pub fn take_recovered_mixer(&mut self) -> Option<Mixer> {
        self.controller.take_recovered_mixer()
    }
    pub fn retry_retirement(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        self.controller.retry_retirement()
    }
    pub fn cancel(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        self.pending = None;
        if let Some(ready) = self.rejected.take() {
            let ReadyOutput { output, hold, .. } = ready;
            let result = self.controller.retire_owned_output(output);
            drop(hold);
            return result.map(|_| true);
        }
        self.controller.stop_owned()
    }
    pub fn stop(&mut self) -> Result<(), ReplacementFailure<B::Error>> {
        let cancellation = self.cancel();
        let retirement = if let Some(output) = self.current.as_mut() {
            self.controller
                .backend_mut()
                .retire(output)
                .map_err(|error| Self::failure(ReplacementPhase::Retire, error))
        } else {
            Ok(())
        };
        cancellation.map(|_| ()).and(retirement)
    }
}
impl<B: OutputReplacementBackend> GameplayOutputOwner<B>
where
    B::Error: std::error::Error + 'static,
{
    pub fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &B::Presentation,
    ) -> Result<Option<EndBoundary>, Box<dyn std::error::Error>> {
        let Some(output) = self.current.as_ref() else {
            return Ok(None);
        };
        if presentation.epoch() != Some(self.controller.backend().epoch(output))
            || self.controller.backend().basis(output) != self.basis
        {
            return Err("current output end identity differs".into());
        }
        let pair = presentation
            .latest_pair()
            .ok_or("current output has no accepted end observation")?;
        self.controller
            .backend()
            .observe_end(output, end, pair, self.report)
    }
    pub fn publish_paused(
        &mut self,
        mut context: GameplayOutputContext<'_, B::Presentation>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.rejected.is_some() {
            return Err(
                "ready output publication was refused; explicit cancellation required".into(),
            );
        }
        if let Some((request, wait_ns)) = self.pending.take() {
            if let Some(output) = self.current.take() {
                if let Err(output) = self.controller.attach(output) {
                    self.current = Some(output);
                    self.pending = Some((request, wait_ns));
                    return Err("replacement controller already owns an output".into());
                }
            }
            self.controller.begin(
                request,
                context.presentation,
                context.pause,
                context.config.song_origin,
                wait_ns,
                || context.control.hold_audio_pause(),
            )?;
        }
        if self.controller.state() != ReplacementState::Waiting {
            return Ok(false);
        }
        let Some(ready) = self.controller.poll(now)? else {
            return Ok(false);
        };
        let basis = ready.timing.basis;
        let report = ready.timing.pause.last_render_report();
        let pair = match ready.timing.presentation.latest_pair() {
            Some(pair) => pair,
            None => {
                self.rejected = Some(ready);
                return Err("ready output has no accepted relation".into());
            }
        };
        let evidence = self
            .controller
            .backend()
            .pause_observation(&ready.output, pair, now);
        let evidence = match evidence {
            Ok(evidence) => evidence,
            Err(error) => {
                self.rejected = Some(ready);
                return Err(Box::new(Self::failure(ReplacementPhase::Observe, error)));
            }
        };
        match publish_ready_output(ready, &mut self.current, context) {
            Ok(()) => {
                self.basis = basis;
                self.report = report;
                self.pause_evidence = Some(evidence);
                Ok(true)
            }
            Err(failure) => {
                self.rejected = Some(failure.ready);
                Err(failure.error)
            }
        }
    }
}
#[cfg(test)]
#[path = "gameplay_output_owner_fixtures.rs"]
pub(crate) mod fixtures;
