//! One static output owner shared by gameplay adapters; input and clocks stay external.
use super::replacement::target::{publish_ready_target_audio_output_held, ReadyTargetAudioOutput};
use crate::gameplay::output::ports::{
    OriginalNativeOutputBackend, OriginalTargetNativeOutputBackend, OutputReplacementBackend,
};
use crate::native_audio_presentation::NativeAudioPresentation;
use crate::{
    gameplay::output::application::replacement::{
        publish_ready_audio_output_held, publish_ready_output, OutputReplacement, ReadyAudioOutput,
        ReadyOutput, ReplacementCause, ReplacementFailure, ReplacementPhase, ReplacementState,
    },
    gameplay_presentation::{GameplayOutputContext, GameplayPresentationPort},
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
};
use beatkernel::audio::{ConvertedRenderReport, TargetFrameBasis};
use beatkernel::{
    audio::{Mixer, OutputFrameBasis, RenderReport, SoftwareOutputState},
    time::{ClockPair, ClockPoint},
};
use beatkernel_platform::audio::presentation::validation::OriginalNativePresentationEvidence;
use beatkernel_platform::audio::{ConvertedBoundaryFacts, ConvertedNativeOutputState};

enum RejectedReady<P: GameplayPresentationPort, O> {
    Legacy(ReadyOutput<P, O>),
    Audio(ReadyAudioOutput<O>),
    Target(ReadyTargetAudioOutput<O>),
}
pub struct GameplayOutputOwner<
    B: OutputReplacementBackend<O, Basis>,
    O = Mixer,
    Basis = OutputFrameBasis,
> {
    current: Option<B::Output>,
    basis: Basis,
    controller: OutputReplacement<B, O, Basis>,
    pending: Option<(B::Request, u64)>,
    rejected: Option<RejectedReady<B::Presentation, B::Output>>,
    report: Option<RenderReport>,
    pause_evidence: Option<LivePauseObservation>,
    converted: Option<ConvertedRenderReport>,
    target_facts: Option<ConvertedBoundaryFacts>,
}
impl<B: OutputReplacementBackend<O, Basis>, O, Basis> GameplayOutputOwner<B, O, Basis> {
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
            converted: None,
            target_facts: None,
        }
    }
    pub fn current(&self) -> Option<&B::Output> {
        self.current.as_ref()
    }
    pub fn current_mut(&mut self) -> Option<&mut B::Output> {
        self.current.as_mut()
    }
    pub fn rejected_ready(&self) -> Option<&ReadyOutput<B::Presentation, B::Output>> {
        match self.rejected.as_ref() {
            Some(RejectedReady::Legacy(ready)) => Some(ready),
            _ => None,
        }
    }
    pub fn rejected_audio_ready(&self) -> Option<&ReadyAudioOutput<B::Output>> {
        match self.rejected.as_ref() {
            Some(RejectedReady::Audio(ready)) => Some(ready),
            _ => None,
        }
    }
    pub fn rejected_target_audio_ready(&self) -> Option<&ReadyTargetAudioOutput<B::Output>> {
        match self.rejected.as_ref() {
            Some(RejectedReady::Target(ready)) => Some(ready),
            _ => None,
        }
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
    pub fn output_clock_suspended(&self) -> bool {
        self.current.is_none()
            && matches!(
                self.controller.state(),
                ReplacementState::Waiting | ReplacementState::PendingRetirement
            )
    }
    pub fn replacement_pending(&self) -> bool {
        self.rejected.is_some()
            || matches!(
                self.controller.state(),
                ReplacementState::Waiting | ReplacementState::PendingRetirement
            )
    }
    pub fn has_work(&self) -> bool {
        self.pending.is_some()
            || self.rejected.is_some()
            || matches!(
                self.controller.state(),
                ReplacementState::Waiting | ReplacementState::PendingRetirement
            )
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
}
impl<B: OutputReplacementBackend<O>, O: SoftwareOutputState> GameplayOutputOwner<B, O> {
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
        self.observe(presentation)?;
        if presentation.latest_pair().is_none() {
            return Err(ReplacementFailure {
                cause: ReplacementCause::Policy(
                    "resume seed has no accepted presentation observation",
                ),
                cleanup: None,
                recovery: None,
            });
        }
        Ok(())
    }
}
impl<B: OutputReplacementBackend<O, Basis>, O, Basis> GameplayOutputOwner<B, O, Basis> {
    pub fn take_recovered_mixer(&mut self) -> Option<O> {
        self.controller.take_recovered_mixer()
    }
    pub fn retry_retirement(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        self.controller.retry_retirement()
    }
    pub fn cancel(&mut self) -> Result<bool, ReplacementFailure<B::Error>> {
        self.pending = None;
        if let Some(ready) = self.rejected.take() {
            let (output, hold) = match ready {
                RejectedReady::Legacy(ReadyOutput { output, hold, .. }) => (output, hold),
                RejectedReady::Audio(ReadyAudioOutput { output, hold, .. }) => (output, hold),
                RejectedReady::Target(ready) => (ready.output, ready.hold),
            };
            let result = self.controller.retire_owned_output_held(output, hold);
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
impl<B: OriginalNativeOutputBackend<O>, O: SoftwareOutputState> GameplayOutputOwner<B, O> {
    fn audio_identity(
        &self,
        presentation: &NativeAudioPresentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        let Some(output) = self.current.as_ref() else {
            return Ok(());
        };
        let epoch = presentation.authority().epoch();
        if epoch.id != self.controller.backend().epoch(output)
            || self.controller.backend().basis(output) != self.basis
            || presentation
                .basis()
                .is_some_and(|basis| basis != self.basis)
        {
            return Err(ReplacementFailure {
                cause: ReplacementCause::Policy("published audio epoch or basis differs"),
                cleanup: None,
                recovery: None,
            });
        }
        let origin = self
            .basis
            .point_at_stream_frame(0)
            .map_err(|error| ReplacementFailure {
                cause: ReplacementCause::Timing(Box::new(error)),
                cleanup: None,
                recovery: None,
            })?;
        if origin != epoch.stream_origin {
            return Err(ReplacementFailure {
                cause: ReplacementCause::Policy("published raw audio origin differs"),
                cleanup: None,
                recovery: None,
            });
        }
        Ok(())
    }
    /// Acquire both IO results before changing application history or the report cache.
    pub fn observe_native(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        self.audio_identity(presentation)?;
        let Some(output) = self.current.as_mut() else {
            return Ok(());
        };
        let snapshot = self
            .controller
            .backend_mut()
            .observe_native(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        let reported = self
            .controller
            .backend()
            .render_report(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        let mut original_report = None;
        if let Some(snapshot) = snapshot {
            if snapshot.epoch != self.controller.backend().epoch(output)
                || snapshot.basis != self.basis
            {
                return Err(ReplacementFailure {
                    cause: ReplacementCause::Timing(
                        "native snapshot differs from published epoch or basis".into(),
                    ),
                    cleanup: None,
                    recovery: None,
                });
            }
            original_report = match snapshot.evidence {
                OriginalNativePresentationEvidence::Asio { observation, .. } => {
                    Some(observation.render)
                }
                OriginalNativePresentationEvidence::Wasapi { snapshot, .. } => snapshot.render,
                OriginalNativePresentationEvidence::SuppliedPair(_) => None,
            };
            presentation
                .admit(snapshot)
                .map_err(|error| ReplacementFailure {
                    cause: ReplacementCause::Timing(error),
                    cleanup: None,
                    recovery: None,
                })?;
        }
        if let Some(report) = original_report.or(reported) {
            self.report = Some(report);
        }
        Ok(())
    }
    /// Preserve full original interval evidence; held outputs reuse only committed pause data.
    pub fn audio_pause_observation(
        &mut self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, ReplacementFailure<B::Error>> {
        self.audio_identity(presentation)?;
        if self.current.is_none() {
            return self.pause_evidence.ok_or(ReplacementFailure {
                cause: ReplacementCause::Policy("waiting output has no committed pause evidence"),
                cleanup: None,
                recovery: None,
            });
        }
        let record = presentation.latest_record().ok_or(ReplacementFailure {
            cause: ReplacementCause::Policy("audio output has no accepted pause evidence"),
            cleanup: None,
            recovery: None,
        })?;
        let evidence = match record.evidence() {
            OriginalNativePresentationEvidence::Asio { observation, .. } => {
                crate::gameplay::output::adapters::observation::asio_pause_observation(
                    Some(*observation),
                    now,
                )
            }
            _ => LivePauseObservation::Point(record.pair()),
        };
        self.pause_evidence = Some(evidence);
        Ok(evidence)
    }
    pub fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &NativeAudioPresentation,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<EndBoundary>>
    where
        B::Error: std::error::Error + 'static,
    {
        self.audio_identity(presentation)?;
        if self.current.is_none() {
            return Ok(None);
        }
        let Some(record) = presentation.latest_record() else {
            return Ok(None);
        };
        match record.evidence() {
            OriginalNativePresentationEvidence::Asio { observation, .. } => {
                Ok(end.observe_asio(*observation)?)
            }
            _ => Ok(end.observe(self.report, record.pair())?),
        }
    }
}
impl<B: OriginalNativeOutputBackend<O>, O: SoftwareOutputState> GameplayOutputOwner<B, O>
where
    B::Error: std::error::Error + 'static,
{
    pub fn publish_paused_audio(
        &mut self,
        mut context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.rejected.is_some() {
            return Err(
                "ready output publication was refused; explicit cancellation required".into(),
            );
        }
        if context.merger.pending() != 0 {
            return Ok(false);
        }
        self.audio_identity(context.presentation)?;
        if let Some((request, wait_ns)) = self.pending.take() {
            if let Some(output) = self.current.take() {
                if let Err(output) = self.controller.attach(output) {
                    self.current = Some(output);
                    self.pending = Some((request, wait_ns));
                    return Err("replacement controller already owns an output".into());
                }
            }
            self.controller.begin_audio(
                request,
                context.presentation,
                context.pause,
                context.merger,
                context.config.song_origin,
                wait_ns,
                || context.control.hold_audio_pause(),
            )?;
        }
        if self.controller.state() != ReplacementState::Waiting {
            return Ok(false);
        }
        let Some(ready) = self
            .controller
            .poll_audio(context.presentation, context.merger, now)?
        else {
            return Ok(false);
        };
        let basis = ready.basis;
        let report = ready.pause.last_render_report();
        let record = ready.prepared.latest_record();
        let evidence = match *record.evidence() {
            OriginalNativePresentationEvidence::Asio { observation, .. } => {
                crate::gameplay::output::adapters::observation::asio_pause_observation(
                    Some(observation),
                    now,
                )
            }
            _ => LivePauseObservation::Point(record.pair()),
        };
        match publish_ready_audio_output_held(ready, &mut self.current, context, now) {
            Ok(hold) => {
                self.basis = basis;
                self.report = report;
                self.pause_evidence = Some(evidence);
                drop(hold);
                Ok(true)
            }
            Err(failure) => {
                self.rejected = Some(RejectedReady::Audio(failure.ready));
                Err(failure.error)
            }
        }
    }
}

impl<B: OutputReplacementBackend<O>, O: SoftwareOutputState> GameplayOutputOwner<B, O>
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
                self.rejected = Some(RejectedReady::Legacy(ready));
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
                self.rejected = Some(RejectedReady::Legacy(ready));
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
                self.rejected = Some(RejectedReady::Legacy(failure.ready));
                Err(failure.error)
            }
        }
    }
}
impl<B: OriginalTargetNativeOutputBackend<ConvertedNativeOutputState>>
    GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis>
{
    fn target_identity(
        &self,
        presentation: &NativeAudioPresentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        let Some(output) = self.current.as_ref() else {
            return Ok(());
        };
        let epoch = presentation.authority().epoch();
        if epoch.id != self.controller.backend().epoch(output)
            || self.controller.backend().basis(output) != self.basis
            || presentation.basis().is_some()
            || presentation
                .target_basis()
                .is_some_and(|basis| basis != self.basis)
            || self
                .basis
                .point_at_stream_frame(0)
                .map_err(|error| ReplacementFailure::timing(Box::new(error)))?
                != epoch.stream_origin
        {
            return Err(ReplacementFailure::policy(
                "published target creation epoch or exact physical basis differs",
            ));
        }
        Ok(())
    }
    /// Collects every fallible IO result before admitting authority or changing caches.
    pub fn observe_target_native(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        self.target_identity(presentation)?;
        let Some(output) = self.current.as_mut() else {
            return Ok(());
        };
        let snapshot = self
            .controller
            .backend_mut()
            .observe_native_target(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        let telemetry = self
            .controller
            .backend()
            .output_telemetry(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        if let Some(telemetry) = telemetry {
            let facts = telemetry.facts;
            if facts.origin != Some(self.basis.origin())
                || facts.source_rate == 0
                || self.target_facts.is_some_and(|old| {
                    old.origin != facts.origin || old.source_rate != facts.source_rate
                })
                || telemetry.converted.is_some_and(|report| {
                    report.target_rate != self.basis.sample_rate()
                        || report.source_rate != facts.source_rate
                })
            {
                return Err(ReplacementFailure::policy(
                    "target observation changed immutable source or target interpretation",
                ));
            }
        }
        if let Some(snapshot) = snapshot {
            if snapshot.epoch != self.controller.backend().epoch(output)
                || snapshot.basis != self.basis
            {
                return Err(ReplacementFailure::policy(
                    "original target snapshot differs from published creation identity",
                ));
            }
            presentation
                .admit_target(snapshot)
                .map_err(ReplacementFailure::timing)?;
        }
        if let Some(telemetry) = telemetry {
            self.report = telemetry.source.filter(|report| report.frames != 0);
            self.converted = telemetry.converted;
            self.target_facts = Some(telemetry.facts);
        }
        Ok(())
    }
    /// Source scheduling evidence, never generated target frame counts.
    pub fn render_report(&self) -> Option<RenderReport> {
        self.report
    }
    pub fn converted_report(&self) -> Option<ConvertedRenderReport> {
        self.converted
    }
    pub fn target_boundary_facts(&self) -> Option<ConvertedBoundaryFacts> {
        self.target_facts
    }
    /// Returns genuine target/native facts to the shared target-aware pause handler.
    /// Waiting output reuses only the previously committed pause observation.
    pub fn audio_pause_observation_target(
        &mut self,
        presentation: &NativeAudioPresentation,
        now: ClockPoint,
    ) -> Result<LivePauseObservation, ReplacementFailure<B::Error>> {
        self.target_identity(presentation)?;
        if now.domain != presentation.authority().epoch().host_domain {
            return Err(ReplacementFailure::policy(
                "target pause observation HOST domain differs",
            ));
        }
        if self.current.is_none() {
            return self.pause_evidence.ok_or_else(|| {
                ReplacementFailure::policy("waiting target output has no committed pause evidence")
            });
        }
        let record = presentation.latest_record().ok_or_else(|| {
            ReplacementFailure::policy("target pause has no accepted original native evidence")
        })?;
        let facts = self.target_facts.ok_or_else(|| {
            ReplacementFailure::policy("target pause source association is unavailable")
        })?;
        let evidence = LivePauseObservation::Target {
            epoch: presentation.authority().epoch().id,
            basis: self.basis,
            source: self.report,
            facts,
            pair: record.pair(),
        };
        self.pause_evidence = Some(evidence);
        Ok(evidence)
    }
    /// Native held mode is separate from producer pause adoption and native acknowledgement.
    pub fn set_target_held(&mut self, held: bool) -> Result<(), ReplacementFailure<B::Error>> {
        let output = self.current.as_mut().ok_or_else(|| {
            ReplacementFailure::policy("target held mode requires a published output")
        })?;
        if self.controller.backend().basis(output) != self.basis {
            return Err(ReplacementFailure::policy(
                "target held output changed creation basis",
            ));
        }
        self.controller
            .backend_mut()
            .set_held(output, held)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))
    }
}
impl<B: OriginalTargetNativeOutputBackend<ConvertedNativeOutputState>>
    GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis>
where
    B::Error: std::error::Error + 'static,
{
    pub fn observe_target_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &NativeAudioPresentation,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<EndBoundary>> {
        self.target_identity(presentation)?;
        if self.current.is_none() {
            return Ok(None);
        }
        let Some(record) = presentation.latest_record() else {
            return Ok(None);
        };
        let Some(facts) = self.target_facts else {
            return Ok(None);
        };
        Ok(end.observe_target(
            presentation.authority().epoch().id,
            self.basis,
            facts,
            self.report,
            record.pair(),
        )?)
    }
    /// Publishes the ready target owner through the same lifecycle and authority transaction.
    pub fn publish_paused_target_audio(
        &mut self,
        mut context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.rejected.is_some() {
            return Err(
                "ready target publication was refused; explicit cancellation required".into(),
            );
        }
        if context.merger.pending() != 0 {
            return Ok(false);
        }
        self.target_identity(context.presentation)?;
        if let Some((request, wait_ns)) = self.pending.take() {
            if let Some(output) = self.current.take() {
                if let Err(output) = self.controller.attach(output) {
                    self.current = Some(output);
                    self.pending = Some((request, wait_ns));
                    return Err("replacement controller already owns an output".into());
                }
            }
            self.controller.begin_target_audio(
                request,
                context.presentation,
                context.pause,
                context.end.as_ref(),
                context.merger,
                context.config.song_origin,
                wait_ns,
                || context.control.hold_audio_pause(),
            )?;
        }
        if self.controller.state() != ReplacementState::Waiting {
            return Ok(false);
        }
        let Some(ready) =
            self.controller
                .poll_target_audio(now, context.presentation, context.merger)?
        else {
            return Ok(false);
        };
        let basis = ready.basis;
        let report = ready.source_report;
        let converted = ready.converted_report;
        let facts = ready.facts;
        match publish_ready_target_audio_output_held(ready, &mut self.current, context, now) {
            Ok(hold) => {
                self.basis = basis;
                self.report = report;
                self.converted = Some(converted);
                self.target_facts = Some(facts);
                self.pause_evidence = None;
                drop(hold);
                Ok(true)
            }
            Err(failure) => {
                self.rejected = Some(RejectedReady::Target(failure.ready));
                Err(failure.error)
            }
        }
    }
}

#[cfg(test)]
#[path = "owner_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "native_observation_fixtures.rs"]
mod native_observation_fixtures;
