//! One static output owner shared by gameplay adapters; input and clocks stay external.
use super::replacement::coordinate_target::{
    CoordinateTargetPoll, publish_ready_coordinate_target_audio_output_held,
};
use super::replacement::target::{ReadyTargetAudioOutput, publish_ready_target_audio_output_held};
use crate::gameplay::output::ports::{
    ConditionalTargetRetirementNativeOutputBackend, NativePlaybackObservation,
    NativeSourcePlaybackQuery, NativeTargetPlaybackObservation, NativeTargetPlaybackQuery,
    OriginalNativeOutputBackend, OriginalTargetNativeOutputBackend, OutputReplacementBackend,
    SourceHistoryRetirementNativeOutputBackend, SourcePlaybackIdentity,
    SourcePlaybackNativeOutputBackend, TargetHistoryRetirementNativeOutputBackend,
    TargetPlaybackNativeOutputBackend, TargetRetirementStatus,
};
use crate::native_audio_presentation::NativeAudioPresentation;
use crate::{
    gameplay::output::application::replacement::{
        OutputReplacement, ReadyAudioOutput, ReadyOutput, ReplacementCause, ReplacementFailure,
        ReplacementPhase, ReplacementState, publish_ready_audio_output_held, publish_ready_output,
    },
    gameplay_presentation::{GameplayOutputContext, GameplayPresentationPort},
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
};
use beatkernel::audio::{ConvertedRenderReport, TargetFrameBasis};
use beatkernel::{
    audio::{
        Mixer, OutputFrameBasis, OutputOpenFailure, PauseHold, RenderReport, SoftwareOutputState,
    },
    time::{ClockPair, ClockPoint},
};
use beatkernel_platform::audio::presentation::validation::{
    NativeObservationAdmission, OriginalNativePresentationEvidence,
};
use beatkernel_platform::audio::{ConvertedBoundaryFacts, ConvertedNativeOutputState};

enum RejectedReady<P: GameplayPresentationPort, O> {
    Legacy(ReadyOutput<P, O>),
    Audio(ReadyAudioOutput<O>),
    Target(ReadyTargetAudioOutput<O>),
    Coordinate { output: O, hold: PauseHold },
}
/// The cold operation returns its staged value without retaining native evidence.
pub enum CoordinateTargetOwnerPublication<A> {
    Pending,
    LiveRefused(String),
    Published(A),
}
pub struct GameplayOutputOwner<
    B: OutputReplacementBackend<O, Basis>,
    O = Mixer,
    Basis = OutputFrameBasis,
    Ticket = (),
> {
    current: Option<B::Output>,
    basis: Basis,
    controller: OutputReplacement<B, O, Basis, Ticket>,
    pending: Option<(B::Request, u64)>,
    rejected: Option<RejectedReady<B::Presentation, B::Output>>,
    report: Option<RenderReport>,
    pause_evidence: Option<LivePauseObservation>,
    converted: Option<ConvertedRenderReport>,
    target_facts: Option<ConvertedBoundaryFacts>,
}
impl<B: OutputReplacementBackend<O, Basis>, O, Basis> GameplayOutputOwner<B, O, Basis> {
    /// Acquires the initial native output through the same owned backend used
    /// for later lifecycle operations. Failure retains the backend's exact
    /// recovered owner, pending output and cleanup evidence.
    pub fn open_initial(
        backend: B,
        request: B::Request,
        complete_owner: O,
        epoch: u64,
    ) -> Result<Self, OutputOpenFailure<B::Error, B::Output, O>> {
        Self::open_initial_with_ticket(backend, request, complete_owner, epoch)
    }
    pub fn new(backend: B, output: B::Output) -> Self {
        Self::new_with_ticket(backend, output)
    }
}
impl<B: OutputReplacementBackend<O, Basis>, O, Basis, Ticket>
    GameplayOutputOwner<B, O, Basis, Ticket>
{
    pub fn open_initial_with_ticket(
        mut backend: B,
        request: B::Request,
        complete_owner: O,
        epoch: u64,
    ) -> Result<Self, OutputOpenFailure<B::Error, B::Output, O>> {
        let output = backend.open(request, complete_owner, epoch)?;
        Ok(Self::new_with_ticket(backend, output))
    }
    pub fn new_with_ticket(backend: B, output: B::Output) -> Self {
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
        self.current
            .as_ref()
            .or_else(|| self.controller.conditional_current())
    }
    pub fn current_mut(&mut self) -> Option<&mut B::Output> {
        if self.current.is_some() {
            self.current.as_mut()
        } else {
            self.controller.conditional_current_mut()
        }
    }
    fn backend_output_mut(&mut self) -> Option<(&mut B, &mut B::Output)> {
        if let Some(output) = self.current.as_mut() {
            Some((self.controller.backend_mut(), output))
        } else {
            self.controller.conditional_backend_output_mut()
        }
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
        self.current()
            .map(|output| self.controller.backend().epoch(output))
            .unwrap_or(0)
            .max(self.controller.last_issued_epoch())
    }
    pub fn output_clock_suspended(&self) -> bool {
        self.current().is_none()
            && (self.rejected.is_some() || self.controller.output_clock_suspended())
    }
    pub fn replacement_pending(&self) -> bool {
        self.rejected.is_some()
            || matches!(
                self.controller.state(),
                ReplacementState::Waiting
                    | ReplacementState::PendingRetirement
                    | ReplacementState::ConditionalRetirement
            )
    }
    pub fn has_work(&self) -> bool {
        self.pending.is_some()
            || self.rejected.is_some()
            || matches!(
                self.controller.state(),
                ReplacementState::Waiting
                    | ReplacementState::PendingRetirement
                    | ReplacementState::ConditionalRetirement
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
impl<B: OutputReplacementBackend<O, Basis>, O, Basis, Ticket>
    GameplayOutputOwner<B, O, Basis, Ticket>
{
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
                RejectedReady::Coordinate { output, hold } => (output, hold),
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
        if context.presentation.has_retained_practice() {
            return Err("output replacement is unavailable during retained practice".into());
        }
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
impl<B: OriginalTargetNativeOutputBackend<ConvertedNativeOutputState>, Ticket>
    GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, Ticket>
{
    fn target_identity(
        &self,
        presentation: &NativeAudioPresentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        let Some(output) = self.current() else {
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
        let basis = self.basis;
        let previous_facts = self.target_facts;
        let Some((backend, output)) = self.backend_output_mut() else {
            return Ok(());
        };
        let snapshot = backend
            .observe_native_target(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        let telemetry = backend
            .output_telemetry(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        if let Some(telemetry) = telemetry {
            let facts = telemetry.facts;
            if facts.origin != Some(basis.origin())
                || facts.source_rate == 0
                || previous_facts.is_some_and(|old| {
                    old.origin != facts.origin || old.source_rate != facts.source_rate
                })
                || telemetry.converted.is_some_and(|report| {
                    report.target_rate != basis.sample_rate()
                        || report.source_rate != facts.source_rate
                })
            {
                return Err(ReplacementFailure::policy(
                    "target observation changed immutable source or target interpretation",
                ));
            }
        }
        if let Some(snapshot) = snapshot {
            if snapshot.epoch != backend.epoch(output) || snapshot.basis != basis {
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
        if self.current().is_none() {
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
        let basis = self.basis;
        let (backend, output) = self.backend_output_mut().ok_or_else(|| {
            ReplacementFailure::policy("target held mode requires a published output")
        })?;
        if backend.basis(output) != basis {
            return Err(ReplacementFailure::policy(
                "target held output changed creation basis",
            ));
        }
        backend
            .set_held(output, held)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))
    }
}
impl<B: OriginalTargetNativeOutputBackend<ConvertedNativeOutputState>, Ticket>
    GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, Ticket>
where
    B::Error: std::error::Error + 'static,
{
    pub fn observe_target_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &NativeAudioPresentation,
    ) -> crate::native_gameplay::NativeGameplayResult<Option<EndBoundary>> {
        self.target_identity(presentation)?;
        if self.current().is_none() {
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
        if context.presentation.has_retained_practice() {
            return Err("output replacement is unavailable during retained practice".into());
        }
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

impl<
    B: crate::gameplay::output::ports::OriginalCoordinateTargetNativeOutputBackend<
            ConvertedNativeOutputState,
        >,
    Ticket,
> GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, Ticket>
{
    /// Acquires original target time evidence without qualifying converted playback.
    /// The separate source report is read before either authority or cache changes.
    pub fn observe_target_native_coordinate(
        &mut self,
        presentation: &mut NativeAudioPresentation,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if self.current().is_none() {
            return Ok(());
        }
        self.target_identity(presentation)?;
        if presentation.authority().coordinate_basis()
            != Some(crate::audio_authority::AudioAuthorityCoordinateBasis::TargetFrame(self.basis))
        {
            return Err(ReplacementFailure::policy(
                "target coordinate requires the published immutable target descriptor",
            ));
        }
        let basis = self.basis;
        let Some((backend, output)) = self.backend_output_mut() else {
            return Ok(());
        };
        let epoch = backend.epoch(output);
        let Some(coordinate) = backend
            .observe_native_target_coordinate(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?
        else {
            return Ok(());
        };
        let reported = backend
            .render_report(output)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        if backend.epoch(output) != epoch
            || backend.basis(output) != basis
            || coordinate.snapshot().epoch != epoch
            || coordinate.snapshot().basis != basis
        {
            return Err(ReplacementFailure::policy(
                "target coordinate acquisition changed the published creation identity",
            ));
        }
        presentation
            .admit_target_coordinate(coordinate)
            .map_err(ReplacementFailure::timing)?;
        self.report = reported.filter(|report| report.frames != 0);
        Ok(())
    }
}

#[cfg(test)]
#[path = "owner_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "native_observation_fixtures.rs"]
mod native_observation_fixtures;

#[cfg(test)]
#[path = "target_coordinate_owner_fixtures.rs"]
mod target_coordinate_owner_fixtures;

#[cfg(test)]
#[path = "source_playback_owner_fixtures.rs"]
pub(crate) mod source_playback_owner_fixtures;

impl<B: SourcePlaybackNativeOutputBackend<O>, O: SoftwareOutputState> GameplayOutputOwner<B, O> {
    fn source_playback_identity(
        &self,
        presentation: &NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        self.audio_identity(presentation)?;
        use crate::audio_authority::AudioAuthorityCoordinateBasis;
        let descriptor_basis = match presentation.authority().coordinate_basis() {
            Some(AudioAuthorityCoordinateBasis::SourceFrame(basis)) => basis,
            Some(AudioAuthorityCoordinateBasis::SourceCounter { basis, frequency })
                if frequency != 0 =>
            {
                basis
            }
            _ => {
                return Err(ReplacementFailure::policy(
                    "source playback requires original source coordinates",
                ));
            }
        };
        if descriptor_basis != self.basis
            || identity.hz() != self.basis.sample_rate()
            || identity.initial_physical() > self.basis.start_physical_frame()
        {
            return Err(ReplacementFailure::policy(
                "source playback creation or history identity differs",
            ));
        }
        Ok(())
    }

    fn source_packet_identity(
        &self,
        packet: &NativeSourcePlaybackQuery<B::NativeEvidence>,
        epoch: u64,
        identity: SourcePlaybackIdentity,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        if packet.epoch() != epoch || packet.basis() != self.basis || packet.identity() != identity
        {
            return Err(ReplacementFailure::policy(
                "source playback packet differs from published identity",
            ));
        }
        Ok(())
    }

    /// Acquires a single compound publication, then admits its original coordinate.
    /// Neither an unavailable packet nor a failed admission changes the report cache.
    pub fn observe_native_playback(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativePlaybackObservation<B::NativeEvidence>>, ReplacementFailure<B::Error>>
    {
        self.observe_native_playback_admission(presentation, identity)
            .map(|observation| observation.map(|(packet, _)| packet))
    }

    fn observe_native_playback_admission(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<
        Option<(
            NativePlaybackObservation<B::NativeEvidence>,
            NativeObservationAdmission,
        )>,
        ReplacementFailure<B::Error>,
    > {
        if self.current.is_none() {
            return Ok(None);
        }
        self.source_playback_identity(presentation, identity)?;
        let Some(output) = self.current.as_mut() else {
            return Ok(None);
        };
        let epoch = self.controller.backend().epoch(output);
        let Some(observation) = self
            .controller
            .backend_mut()
            .observe_native_playback(output, identity)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?
        else {
            return Ok(None);
        };
        if self.controller.backend().epoch(output) != epoch
            || self.controller.backend().basis(output) != self.basis
        {
            return Err(ReplacementFailure::policy(
                "source acquisition changed the published output identity",
            ));
        }
        self.source_packet_identity(observation.source(), epoch, identity)?;
        // The packet constructor checks exact coordinate/query equality. The
        // presentation owns native validation and transactional clock admission.
        let admission = presentation
            .admit_coordinate(observation.coordinate())
            .map_err(ReplacementFailure::timing)?;
        self.report = observation.source().facts().render;
        Ok(Some((observation, admission)))
    }

    /// Acquire and admit one compound source publication for later control staging.
    /// No separate render/native-position read or acknowledgment occurs here.
    pub fn observe_native_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<
        Option<crate::native_coordinate_control::CoordinateControlFacts>,
        ReplacementFailure<B::Error>,
    > {
        let Some((observation, admission)) =
            self.observe_native_playback_admission(presentation, identity)?
        else {
            return Ok(None);
        };
        // A later HOST on an unchanged native position was not retained. An
        // exact retained repeat may still carry newer render/control summaries.
        if admission == NativeObservationAdmission::Unchanged
            && presentation.latest_record().is_some_and(|record| {
                *record.evidence() != observation.coordinate().snapshot().evidence
            })
        {
            return Ok(None);
        }
        crate::native_coordinate_control::CoordinateControlFacts::from_source(
            &observation,
            presentation,
            identity,
        )
        .map(Some)
        .map_err(|error| ReplacementFailure::timing(error.into()))
    }

    /// Reads exact source facts without changing presentation or cached reports.
    pub fn source_playback_at(
        &self,
        presentation: &NativeAudioPresentation,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeSourcePlaybackQuery<B::NativeEvidence>>, ReplacementFailure<B::Error>>
    {
        let Some(output) = self.current.as_ref() else {
            return Ok(None);
        };
        self.source_playback_identity(presentation, identity)?;
        let epoch = self.controller.backend().epoch(output);
        let Some(packet) = self
            .controller
            .backend()
            .source_playback_at(output, query, identity)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?
        else {
            return Ok(None);
        };
        self.source_packet_identity(&packet, epoch, identity)?;
        if self.controller.backend().epoch(output) != epoch
            || self.controller.backend().basis(output) != self.basis
        {
            return Err(ReplacementFailure::policy(
                "source query changed the published output identity",
            ));
        }
        if !packet.query().compare_interpolated_time(*query).is_eq() {
            return Err(ReplacementFailure::policy(
                "source playback packet changed the exact query",
            ));
        }
        Ok(Some(packet))
    }
}

#[cfg(test)]
#[path = "source_control_owner_fixtures.rs"]
pub(crate) mod source_control_owner_fixtures;

impl<B: SourceHistoryRetirementNativeOutputBackend<O>, O: SoftwareOutputState>
    GameplayOutputOwner<B, O>
{
    /// Queues the original covering span for a caller-qualified live minimum.
    /// The caller must include all input, control and practice references.
    pub fn request_source_history_retirement(
        &mut self,
        presentation: &NativeAudioPresentation,
        minimum_query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<u64>, ReplacementFailure<B::Error>> {
        self.request_source_history_retirement_with_source_pin(
            presentation,
            minimum_query,
            identity,
            None,
        )
    }

    /// Includes an original queued or held raw Source reference without rebasing.
    pub fn request_source_history_retirement_with_source_pin(
        &mut self,
        presentation: &NativeAudioPresentation,
        minimum_query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
        source_minimum: Option<u64>,
    ) -> Result<Option<u64>, ReplacementFailure<B::Error>> {
        if source_minimum.is_some_and(|minimum| minimum < identity.initial_physical()) {
            return Err(ReplacementFailure::policy(
                "source history retirement minimum precedes the original source identity",
            ));
        }
        let Some(packet) = self.source_playback_at(presentation, minimum_query, identity)? else {
            return Ok(None);
        };
        if packet.native_evidence().is_none() {
            return Err(ReplacementFailure::policy(
                "source history retirement requires native source evidence",
            ));
        }
        let covering_start = packet.facts().playback.physical_start();
        let source_pin = source_minimum.map_or(covering_start, |pin| pin.min(covering_start));
        let Some(output) = self.current.as_mut() else {
            return Ok(None);
        };
        if self.controller.backend().epoch(output) != packet.epoch()
            || self.controller.backend().basis(output) != packet.basis()
        {
            return Err(ReplacementFailure::policy(
                "source history retirement changed the published output identity",
            ));
        }
        self.controller
            .backend_mut()
            .request_source_history_retirement(
                output,
                packet.epoch(),
                packet.basis(),
                identity,
                source_pin,
            )
            .map_err(|error| Self::failure(ReplacementPhase::Retire, error))
    }
}

#[cfg(test)]
#[path = "source_history_retirement_owner_fixtures.rs"]
mod source_history_retirement_owner_fixtures;

impl<B: TargetPlaybackNativeOutputBackend<ConvertedNativeOutputState>, Ticket>
    GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, Ticket>
{
    fn target_playback_identity(
        &self,
        presentation: &NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        self.target_identity(presentation)?;
        presentation
            .validate_coordinate_seed()
            .map_err(ReplacementFailure::timing)?;
        if presentation.authority().coordinate_basis()
            != Some(crate::audio_authority::AudioAuthorityCoordinateBasis::TargetFrame(self.basis))
            || self.target_facts.is_some_and(|old| {
                old.origin != Some(self.basis.origin()) || old.source_rate != identity.hz()
            })
        {
            return Err(ReplacementFailure::policy(
                "target playback creation descriptor or immutable source interpretation differs",
            ));
        }
        Ok(())
    }

    fn target_packet_identity(
        &self,
        packet: &NativeTargetPlaybackQuery<B::NativeEvidence>,
        epoch: u64,
        identity: SourcePlaybackIdentity,
    ) -> Result<(), ReplacementFailure<B::Error>> {
        let facts = packet.facts().boundaries;
        if packet.epoch() != epoch
            || packet.basis() != self.basis
            || packet.identity() != identity
            || self.target_facts.is_some_and(|old| {
                old.origin != facts.origin || old.source_rate != facts.source_rate
            })
        {
            return Err(ReplacementFailure::policy(
                "target playback packet differs from published creation identity",
            ));
        }
        Ok(())
    }

    /// Admits the original coordinate from one compound converted publication.
    /// Reports and boundaries become visible only after successful admission.
    pub fn observe_target_native_playback(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<
        Option<NativeTargetPlaybackObservation<B::NativeEvidence>>,
        ReplacementFailure<B::Error>,
    > {
        self.observe_target_native_playback_admission(presentation, identity)
            .map(|observation| observation.map(|(packet, _)| packet))
    }

    fn observe_target_native_playback_admission(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<
        Option<(
            NativeTargetPlaybackObservation<B::NativeEvidence>,
            NativeObservationAdmission,
        )>,
        ReplacementFailure<B::Error>,
    > {
        if self.current().is_none() {
            return Ok(None);
        }
        self.target_playback_identity(presentation, identity)?;
        let basis = self.basis;
        let Some((backend, output)) = self.backend_output_mut() else {
            return Ok(None);
        };
        let epoch = backend.epoch(output);
        let observation = backend
            .observe_native_target_playback(output, identity)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        if backend.epoch(output) != epoch || backend.basis(output) != basis {
            return Err(ReplacementFailure::policy(
                "target playback acquisition changed the published output identity",
            ));
        }
        let Some(observation) = observation else {
            return Ok(None);
        };
        self.target_packet_identity(observation.playback(), epoch, identity)?;
        let admission = self.admit_target_playback(presentation, &observation)?;
        Ok(Some((observation, admission)))
    }

    fn admit_target_playback(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        observation: &NativeTargetPlaybackObservation<B::NativeEvidence>,
    ) -> Result<NativeObservationAdmission, ReplacementFailure<B::Error>> {
        let admission = presentation
            .admit_target_coordinate(observation.coordinate())
            .map_err(ReplacementFailure::timing)?;
        let facts = observation.playback().facts();
        self.report = facts.source.filter(|report| report.frames != 0);
        self.converted = facts.converted;
        self.target_facts = Some(facts.boundaries);
        Ok(admission)
    }

    /// Acquires target control facts only from retained original native evidence.
    pub fn observe_target_native_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> Result<
        Option<crate::native_coordinate_control::TargetCoordinateControlFacts>,
        ReplacementFailure<B::Error>,
    > {
        let Some((observation, admission)) =
            self.observe_target_native_playback_admission(presentation, identity)?
        else {
            return Ok(None);
        };
        self.target_control_from_admission(presentation, identity, &observation, admission)
    }

    fn target_control_from_admission(
        &self,
        presentation: &NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
        observation: &NativeTargetPlaybackObservation<B::NativeEvidence>,
        admission: NativeObservationAdmission,
    ) -> Result<
        Option<crate::native_coordinate_control::TargetCoordinateControlFacts>,
        ReplacementFailure<B::Error>,
    > {
        // Preserve exact retained repeats while waiting on unaccepted HOST evidence.
        if admission == NativeObservationAdmission::Unchanged
            && presentation.latest_record().is_some_and(|record| {
                *record.evidence() != observation.coordinate().snapshot().evidence
            })
        {
            return Ok(None);
        }
        crate::native_coordinate_control::TargetCoordinateControlFacts::from_target(
            observation,
            presentation,
            identity,
        )
        .map(Some)
        .map_err(|error| ReplacementFailure::timing(error.into()))
    }

    /// Reads one exact converted packet without admitting or caching its facts.
    pub fn target_playback_at(
        &self,
        presentation: &NativeAudioPresentation,
        query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<NativeTargetPlaybackQuery<B::NativeEvidence>>, ReplacementFailure<B::Error>>
    {
        let Some(output) = self.current() else {
            return Ok(None);
        };
        self.target_playback_identity(presentation, identity)?;
        let epoch = self.controller.backend().epoch(output);
        let packet = self
            .controller
            .backend()
            .target_playback_at(output, query, identity)
            .map_err(|error| Self::failure(ReplacementPhase::Observe, error))?;
        if self.controller.backend().epoch(output) != epoch
            || self.controller.backend().basis(output) != self.basis
        {
            return Err(ReplacementFailure::policy(
                "target playback query changed the published output identity",
            ));
        }
        let Some(packet) = packet else {
            return Ok(None);
        };
        self.target_packet_identity(&packet, epoch, identity)?;
        if !packet.query().compare_interpolated_time(*query).is_eq() {
            return Err(ReplacementFailure::policy(
                "target playback packet changed the exact query",
            ));
        }
        Ok(Some(packet))
    }
}

#[cfg(test)]
#[path = "target_playback_owner_fixtures.rs"]
mod target_playback_owner_fixtures;

impl<B: TargetHistoryRetirementNativeOutputBackend<ConvertedNativeOutputState>, Ticket>
    GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, Ticket>
{
    /// Queues the original covering spans for a caller-qualified live minimum.
    /// The caller must include every live input, control and practice reference.
    pub fn request_target_history_retirement(
        &mut self,
        presentation: &NativeAudioPresentation,
        minimum_query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> Result<Option<u64>, ReplacementFailure<B::Error>> {
        self.request_target_history_retirement_with_source_pin(
            presentation,
            minimum_query,
            identity,
            None,
        )
    }

    /// Includes an original queued or held Source reference in the qualified minimum.
    pub fn request_target_history_retirement_with_source_pin(
        &mut self,
        presentation: &NativeAudioPresentation,
        minimum_query: &beatkernel::audio::InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
        source_minimum: Option<u64>,
    ) -> Result<Option<u64>, ReplacementFailure<B::Error>> {
        if source_minimum.is_some_and(|minimum| minimum < identity.initial_physical()) {
            return Err(ReplacementFailure::policy(
                "target history retirement source minimum precedes the original source identity",
            ));
        }
        let Some(packet) = self.target_playback_at(presentation, minimum_query, identity)? else {
            return Ok(None);
        };
        let facts = packet.facts().playback;
        let covering_source_start = facts.playback.physical_start();
        let source_pin = source_minimum
            .map(|minimum| minimum.min(covering_source_start))
            .unwrap_or(covering_source_start);
        let words = facts.phase.to_words();
        let target_pin = beatkernel::audio::TargetTime::new(words[4], words[5], words[6])
            .map_err(|error| ReplacementFailure::timing(Box::new(error)))?;
        let Some((backend, output)) = self.backend_output_mut() else {
            return Ok(None);
        };
        if backend.epoch(output) != packet.epoch() || backend.basis(output) != packet.basis() {
            return Err(ReplacementFailure::policy(
                "target history retirement changed the published output identity",
            ));
        }
        backend
            .request_target_history_retirement(
                output,
                packet.epoch(),
                packet.basis(),
                identity,
                source_pin,
                target_pin,
            )
            .map_err(|error| Self::failure(ReplacementPhase::Retire, error))
    }
}

#[cfg(test)]
#[path = "target_history_retirement_owner_fixtures.rs"]
mod target_history_retirement_owner_fixtures;

#[derive(Debug)]
struct ConditionalTargetObservationFailure<E> {
    observation: E,
    probe: ReplacementFailure<E>,
}
impl<E: std::fmt::Display> std::fmt::Display for ConditionalTargetObservationFailure<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "native output observation: {}; retirement probe: {}",
            self.observation, self.probe
        )
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ConditionalTargetObservationFailure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.observation)
    }
}

impl<B: ConditionalTargetRetirementNativeOutputBackend<ConvertedNativeOutputState>>
    GameplayOutputOwner<B, ConvertedNativeOutputState, TargetFrameBasis, B::Ticket>
where
    B::Error: std::error::Error + 'static,
{
    pub fn publish_paused_coordinate_target_audio<A>(
        &mut self,
        mut context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        identity: SourcePlaybackIdentity,
        practice_pending: bool,
        stage: impl FnOnce(&B::Output) -> Result<A, Box<dyn std::error::Error>>,
    ) -> crate::native_gameplay::NativeGameplayResult<CoordinateTargetOwnerPublication<A>> {
        if self.rejected.is_some() {
            return Err(
                "ready coordinate publication was refused; explicit cancellation required".into(),
            );
        }
        self.target_identity(context.presentation)?;
        if self.pending.is_some() && context.merger.pending() != 0 {
            return Ok(CoordinateTargetOwnerPublication::Pending);
        }
        if let Some((request, wait_ns)) = self.pending.take() {
            if practice_pending || context.end.is_some() {
                return Ok(CoordinateTargetOwnerPublication::LiveRefused(
                    "coordinate replacement requires no NativeEnd or pending practice".into(),
                ));
            }
            if let Some(output) = self.current.take() {
                if let Err(output) = self.controller.attach(output) {
                    self.current = Some(output);
                    self.pending = Some((request, wait_ns));
                    return Err("replacement controller already owns an output".into());
                }
            }
            if let Err(failure) = self.controller.begin_coordinate_target_audio(
                request,
                context.presentation,
                context.pause,
                context.end.as_ref(),
                context.merger,
                identity,
                practice_pending,
                wait_ns,
                || context.control.hold_audio_pause(),
            ) {
                let restored = self.controller.take_attached_output();
                let cold = restored.is_some()
                    && matches!(
                        &failure.cause,
                        ReplacementCause::Policy(_) | ReplacementCause::Timing(_)
                    );
                self.current = restored;
                if cold {
                    return Ok(CoordinateTargetOwnerPublication::LiveRefused(
                        failure.to_string(),
                    ));
                }
                return Err(Box::new(failure));
            }
        }
        if !matches!(
            self.controller.state(),
            ReplacementState::ConditionalRetirement | ReplacementState::Waiting
        ) {
            return Ok(CoordinateTargetOwnerPublication::Pending);
        }
        match self.controller.poll_coordinate_target_audio(
            now,
            context.presentation,
            context.pause,
            context.merger,
        )? {
            CoordinateTargetPoll::Pending => Ok(CoordinateTargetOwnerPublication::Pending),
            CoordinateTargetPoll::LiveRefused(reason) => {
                self.current = self.controller.take_attached_output();
                Ok(CoordinateTargetOwnerPublication::LiveRefused(reason.into()))
            }
            CoordinateTargetPoll::Ready(ready) => {
                let basis = ready.basis();
                let telemetry = ready.telemetry();
                match publish_ready_coordinate_target_audio_output_held(
                    ready,
                    &mut self.current,
                    context,
                    now,
                    |ready| stage(ready.output()),
                ) {
                    Ok((hold, applied)) => {
                        self.basis = basis;
                        self.report = telemetry.source.filter(|report| report.frames != 0);
                        self.converted = telemetry.converted;
                        self.target_facts = Some(telemetry.facts);
                        self.pause_evidence = None;
                        drop(hold);
                        Ok(CoordinateTargetOwnerPublication::Published(applied))
                    }
                    Err(failure) => {
                        let (output, hold) = failure.ready.into_output_and_hold();
                        self.rejected = Some(RejectedReady::Coordinate { output, hold });
                        Err(failure.error)
                    }
                }
            }
        }
    }

    fn observe_conditional_target_native_playback_admission(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> crate::native_gameplay::NativeGameplayResult<
        Option<(
            NativeTargetPlaybackObservation<B::NativeEvidence>,
            NativeObservationAdmission,
        )>,
    > {
        if self.controller.probe_coordinate_target_retirement()?
            == Some(TargetRetirementStatus::Authorized)
            || self.current().is_none()
        {
            return Ok(None);
        }
        self.target_playback_identity(presentation, identity)?;
        let basis = self.basis;
        let (epoch, observed, same_identity) = {
            let Some((backend, output)) = self.backend_output_mut() else {
                return Ok(None);
            };
            let epoch = backend.epoch(output);
            let observed = backend.observe_native_target_playback(output, identity);
            let same_identity = backend.epoch(output) == epoch && backend.basis(output) == basis;
            (epoch, observed, same_identity)
        };
        let observation = match observed {
            Ok(observation) => observation,
            Err(observation) => {
                return match self.controller.probe_coordinate_target_retirement() {
                    Ok(Some(TargetRetirementStatus::Authorized)) => Ok(None),
                    Ok(_) => Err(Box::new(Self::failure(
                        ReplacementPhase::Observe,
                        observation,
                    ))),
                    Err(probe) => Err(Box::new(ConditionalTargetObservationFailure {
                        observation,
                        probe,
                    })),
                };
            }
        };
        if !same_identity {
            return Err(Box::new(ReplacementFailure::<B::Error>::policy(
                "target playback acquisition changed the published output identity",
            )));
        }
        let Some(observation) = observation else {
            return Ok(None);
        };
        self.target_packet_identity(observation.playback(), epoch, identity)?;
        let admission = self.admit_target_playback(presentation, &observation)?;
        Ok(Some((observation, admission)))
    }

    pub fn observe_conditional_target_native_playback(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> crate::native_gameplay::NativeGameplayResult<
        Option<NativeTargetPlaybackObservation<B::NativeEvidence>>,
    > {
        self.observe_conditional_target_native_playback_admission(presentation, identity)
            .map(|observation| observation.map(|(packet, _)| packet))
    }

    pub fn observe_conditional_target_native_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> crate::native_gameplay::NativeGameplayResult<
        Option<crate::native_coordinate_control::TargetCoordinateControlFacts>,
    > {
        let Some((observation, admission)) =
            self.observe_conditional_target_native_playback_admission(presentation, identity)?
        else {
            return Ok(None);
        };
        Ok(self.target_control_from_admission(presentation, identity, &observation, admission)?)
    }
}

#[cfg(test)]
#[path = "retained_target_observation_fixtures.rs"]
mod retained_target_observation_fixtures;
