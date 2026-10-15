//! Nonblocking control-side ownership of actual solo/local runtimes and one BGM queue.
//! Hosts supply clock relations and genuine completed-render cursors. This
//! module does not sample a platform clock or infer delivery from admission.
use crate::{
    audio_authority::{
        AudioAuthority, AudioAuthorityCoordinateBasis, AudioAuthorityError, PreparedFrontier,
        PreparedInput,
    },
    bgm::{BgmConfig, BgmFeedError, BgmFeedReport, BgmFeeder},
    competition::{CompetitionError, ScoreSummary},
    completion::{CompletionError, SongCompletion},
    gauge::{BmsGauge, GaugeError},
    input_sounds::{InputSoundIdentity, InputSoundPlan},
    live_speed::LiveSpeedSetup,
    local_input::InputMerger,
    local_players::{PlayerId, ResolvedInputPlan},
    local_preparation::{
        prepare_local_input_sounds, prepare_local_members_with_timing, prepare_local_mine_sounds,
        PreparedLocalMembers,
    },
    local_runtime::{GroupError, InputResult, PlayerReport, RuntimeGroup, SoloRuntime},
    mine_damage::{MineDamageError, MineDamageSummary},
    mine_plan::prepare_judge_with_timing,
    mine_sounds::MineSoundPlan,
    native_judge::NativeJudgeConfig,
    offline::OwnedStopEvidence,
    play_result::CompletedPlayResult,
    practice_control::{
        PracticeAction, PracticeApplied, PracticeCapability, PracticeRateState, PracticeReply,
        PracticeTransactionCapability, PracticeTransactionReply, PracticeTransactionRequest,
    },
    practice_playback::{PracticeRuntimeHeader, PreparedRuntimeRateRequest, RuntimePractice},
    replay_audio::{
        before_endpoint, completed_render_cursor_with_stops, section_end_frame, ReplayAudioError,
    },
    replay_capture::{setup_gauge_header, CaptureError, LiveReplayCapture},
    replay_speed::ReplaySpeed,
    section_start::PreparedRetainedSection,
    worklet_audio::{WorkletRetainedConfig, WorkletRetainedEnvelope, WorkletRetainedEvent},
    PreparedBms,
};
use beatkernel::{
    audio::{
        command_queue, AudioCommand, AudioCounters, AudioLimits, CommandConsumer, CommandPushError,
        CommandScope, InterpolatedTargetTime, OutputFrameBasis, PracticeBoundaryKind, PracticeCue,
        PracticeEvent, PracticeRateCueGeometry, PracticeRateOutcome, PracticeRatePreparation,
        PracticeRegion, QueuePopError, RenderReport, SampleBank, SourcePlaybackSpan,
        SourcePlaybackState, TargetTime,
    },
    chart::ObjectId,
    input::{BindingMap, PhysicalInputEvent, Position2, TouchRegion, TouchRouter},
    interaction::InteractionState,
    judge::JudgeEngine,
    replay::{codec::ReplayCodecLimits, ReplayHeader},
    runtime::{
        hazard_sound::HazardSoundTimeline, input_sound::InputSoundTimeline, RuntimeProcessingClock,
        RuntimeReport,
    },
    time::{ClockDomainId, ClockMapper, ClockPair, ClockPoint, Duration, Timestamp},
    transport::{OutputPhaseBasis, OutputPhasePoint, PositiveRatePhase, Rate, Transport},
};
use beatkernel_bms::BmsInputMode;
use beatkernel_platform::audio::presentation::discipline::{
    DisciplineConfig, DisciplineError, DisciplineUpdate, ObservationAdmission,
    PresentationDiscipline,
};
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug)]
pub struct StepRetainedOptions {
    pub worklet: WorkletRetainedConfig,
    pub chart_seed: u64,
    pub identity_limits: ReplayCodecLimits,
}
#[derive(Clone, Debug)]
pub struct StepRetainedSetup {
    pub config: WorkletRetainedConfig,
    pub cues: Vec<PracticeCue>,
    pub geometry: Vec<PracticeRateCueGeometry>,
    pub sample_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRemoteHistorySnapshot {
    pub program_id: u64,
    pub context_start: u64,
    pub source_hz: u32,
    pub initial_physical: u64,
    pub initial_playback: u64,
    pub retained_start: u64,
    pub pulled_end: u64,
    pub retirement_revision: u64,
    pub spans: Vec<SourcePlaybackSpan>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepRetainedControl {
    pub request: PracticeTransactionRequest,
    pub preparation: Option<PracticeRatePreparation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepRetainedAdmissionKind {
    Transaction,
    Pause,
}

struct RetainedPending {
    request: PracticeTransactionRequest,
    region: PracticeRegion,
    rate: Option<PreparedRuntimeRateRequest>,
    admitted: bool,
}
struct StepRetained {
    setup: StepRetainedSetup,
    original: beatkernel_bms::BmsChart,
    options: StepRetainedOptions,
    business: RuntimePractice,
    phase: PositiveRatePhase,
    headers: Vec<PracticeRuntimeHeader>,
    capture_limits: Vec<Option<ReplayCodecLimits>>,
    completion_template: SongCompletion,
    basis: OutputPhaseBasis,
    region: PracticeRegion,
    generation: u64,
    iteration: u64,
    started: bool,
    ended: bool,
    paused: bool,
    pending: Option<RetainedPending>,
    pause: Option<(u64, u64, bool, bool)>,
    history: Option<StepRemoteHistorySnapshot>,
    event: Option<(WorkletRetainedEnvelope, bool)>,
    acknowledged: Option<WorkletRetainedEnvelope>,
    response: Option<PracticeTransactionReply>,
    retired: Vec<(PlayerId, LiveReplayCapture)>,
}

fn retained_error(error: impl fmt::Display) -> StepGameplayError {
    StepGameplayError::Setup(error.to_string())
}
fn retained_source_error(error: beatkernel::audio::ConvertedPhaseError) -> StepGameplayError {
    StepGameplayError::Setup(format!("retained source coordinate: {error:?}"))
}
fn transaction_identity(request: PracticeTransactionRequest) -> (u64, u64) {
    match request {
        PracticeTransactionRequest::Region(request) => (request.id, request.generation),
        PracticeTransactionRequest::Rate { id, generation, .. }
        | PracticeTransactionRequest::RestartOriginal { id, generation, .. } => (id, generation),
    }
}

impl StepRemoteHistorySnapshot {
    pub(crate) fn from_words(words: &[u32]) -> Result<Self, StepGameplayError> {
        let invalid = || StepGameplayError::InvalidConfiguration("invalid retained history wire");
        if words.len() < 20 || words.len() % 2 != 0 {
            return Err(invalid());
        }
        let pair =
            |index: usize| u64::from(words[index * 2]) | (u64::from(words[index * 2 + 1]) << 32);
        let count = usize::try_from(pair(9)).map_err(|_| invalid())?;
        if pair(0) != 1
            || count > 65_536
            || count.checked_mul(10).and_then(|n| n.checked_add(20)) != Some(words.len())
        {
            return Err(invalid());
        }
        let mut spans = Vec::new();
        spans
            .try_reserve_exact(count)
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        for index in 0..count {
            let at = 10 + index * 5;
            let end = match pair(at + 1) {
                0 if pair(at + 2) == 0 => None,
                1 => Some(pair(at + 2)),
                _ => return Err(invalid()),
            };
            let state = match pair(at + 4) {
                0 => SourcePlaybackState::Frozen,
                1 => SourcePlaybackState::Running,
                _ => return Err(invalid()),
            };
            spans.push(
                SourcePlaybackSpan::new(pair(at), end, pair(at + 3), state)
                    .map_err(retained_source_error)?,
            );
        }
        Ok(Self {
            program_id: pair(1),
            context_start: pair(2),
            source_hz: u32::try_from(pair(3)).map_err(|_| invalid())?,
            initial_physical: pair(4),
            initial_playback: pair(5),
            retained_start: pair(6),
            pulled_end: pair(7),
            retirement_revision: pair(8),
            spans,
        })
    }
    fn span_at(
        &self,
        query: InterpolatedTargetTime,
    ) -> Result<SourcePlaybackSpan, StepGameplayError> {
        if query
            .compare_source_time_frame(self.retained_start, self.source_hz)
            .map_err(retained_source_error)?
            .is_lt()
            || query
                .compare_source_time_frame(self.pulled_end, self.source_hz)
                .map_err(retained_source_error)?
                .is_gt()
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "query outside retained source pulls",
            ));
        }
        let mut low = 0;
        let mut high = self.spans.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if query
                .compare_source_time_frame(self.spans[mid].physical_start(), self.source_hz)
                .map_err(retained_source_error)?
                .is_lt()
            {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        low.checked_sub(1)
            .and_then(|index| self.spans.get(index).copied())
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained source history is empty",
            ))
    }
}

impl StepRetained {
    fn commit_control(
        &mut self,
        event: WorkletRetainedEnvelope,
        point: OutputPhasePoint,
        transport: &mut Transport,
        captures: &mut [(PlayerId, &mut Option<LiveReplayCapture>)],
    ) -> Result<(), StepGameplayError> {
        match event.event {
            WorkletRetainedEvent::Practice(projected) => {
                let PracticeEvent::Rate(rate) = projected.event else {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "not a retained rate event",
                    ));
                };
                let pending = self
                    .pending
                    .as_ref()
                    .filter(|pending| pending.admitted)
                    .ok_or(StepGameplayError::InvalidConfiguration(
                        "rate outcome lacks admitted request",
                    ))?;
                let (id, generation) = transaction_identity(pending.request);
                if !self.started
                    || rate.generation != self.generation
                    || rate.iteration != self.iteration
                    || rate.request_id != id
                {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "rate outcome differs from actual generation or iteration",
                    ));
                }
                let reservation = pending
                    .rate
                    .as_ref()
                    .ok_or(StepGameplayError::InvalidConfiguration(
                        "rate reservation is missing",
                    ))?
                    .clone();
                let phase = if rate.outcome == PracticeRateOutcome::Applied {
                    self.phase
                        .with_rate_at_elapsed(rate.active_elapsed_nanos, rate.applied_rate)
                        .map_err(retained_error)?
                } else {
                    self.phase
                };
                let prepared = self
                    .business
                    .prepare_rate(rate, point, transport, captures, reservation)
                    .map_err(retained_error)?;
                self.business.apply_rate(prepared).map_err(retained_error)?;
                self.phase = phase;
                self.response = Some(PracticeTransactionReply::Rate {
                    id,
                    generation,
                    result: Ok(rate),
                });
                self.pending = None;
            }
            WorkletRetainedEvent::Pause(pause) => {
                if self.pause != Some((pause.request_id, pause.generation, pause.paused, true))
                    || pause.generation != self.generation
                    || !self.started
                {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "actual pause differs from admitted request",
                    ));
                }
                let mut candidate = transport.clone();
                candidate
                    .try_reserve_output_phase_controls(1)
                    .map_err(retained_error)?;
                if pause.paused {
                    candidate.pause_at_output_phase(point)
                } else {
                    candidate.resume_at_output_phase(point)
                }
                .map_err(retained_error)?;
                *transport = candidate;
                self.paused = pause.paused;
                self.pause = None;
            }
        }
        Ok(())
    }
    fn commit_boundary(
        &mut self,
        update: &RetainedBoundaryUpdate,
    ) -> Result<(), StepGameplayError> {
        if !update.fresh && update.rate != self.business.rate_state() {
            self.business
                .commit_control_revision(self.business.rate_state(), update.rate.revision)
                .map_err(retained_error)?;
        }
        self.region = update.region;
        self.generation = update.receipt.generation;
        self.iteration = update.receipt.iteration;
        self.started = true;
        self.ended = update.receipt.kind == PracticeBoundaryKind::Ended;
        if update.fresh {
            self.paused = false;
        }
        if update.receipt.request_id != 0 {
            self.pending = None;
            self.response = update.response.clone();
        }
        Ok(())
    }
    fn permits_query(&self, query: InterpolatedTargetTime) -> Result<bool, StepGameplayError> {
        match self.event {
            Some((event, false)) => Ok(!query
                .compare_interpolated_time(retained_event_query(event, self.basis.source_hz())?)
                .is_gt()),
            _ => Ok(true),
        }
    }
    fn phase_point(
        &self,
        logical: ClockPoint,
        query: InterpolatedTargetTime,
    ) -> Result<OutputPhasePoint, StepGameplayError> {
        let history = self
            .history
            .as_ref()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained history is unavailable",
            ))?;
        let elapsed = history
            .span_at(query)?
            .elapsed_nanos_at_source_time(
                query,
                self.basis.source_hz(),
                self.basis.active_origin_frame(),
            )
            .map_err(retained_source_error)?;
        self.basis
            .point_from_elapsed_nanos(logical, elapsed)
            .map_err(retained_error)
    }
    fn capability(&self) -> Option<PracticeTransactionCapability> {
        (self.started && !self.ended).then(|| PracticeTransactionCapability {
            practice: PracticeCapability {
                generation: self.generation,
                min_target: Timestamp::ZERO,
                max_target: self.setup.config.initial_region.end,
            },
            rate: self.business.rate_state(),
        })
    }
    fn prepare(
        &mut self,
        request: PracticeTransactionRequest,
        fixed: bool,
        transport: &mut Transport,
        captures: &mut [(PlayerId, &mut Option<LiveReplayCapture>)],
    ) -> Result<StepRetainedControl, StepGameplayError> {
        if self.pending.is_some()
            || self.pause.is_some()
            || self.event.is_some()
            || self.response.is_some()
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained controls are busy",
            ));
        }
        let capability = self
            .capability()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained controls are busy or unavailable",
            ))?;
        let (id, generation) = transaction_identity(request);
        if fixed || id == 0 || generation != capability.practice.generation {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained control conflicts with competition or generation",
            ));
        }
        let mut region = self.region;
        let rate = match request {
            PracticeTransactionRequest::Rate {
                expected,
                requested_rate,
                ..
            } => {
                if expected != capability.rate {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "stale retained rate state",
                    ));
                }
                Some(requested_rate)
            }
            PracticeTransactionRequest::RestartOriginal { expected, .. } => {
                if expected != capability.rate {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "stale retained restart state",
                    ));
                }
                region = self.setup.config.initial_region;
                Some(self.setup.config.initial_rate)
            }
            PracticeTransactionRequest::Region(request) => {
                region = match request.action {
                    PracticeAction::Scrub { target } => {
                        PracticeRegion::new(target, self.setup.config.initial_region.end, false)
                    }
                    PracticeAction::Loop { start, end } => PracticeRegion::new(start, end, true),
                    PracticeAction::DisableLoop => {
                        PracticeRegion::new(self.region.start, self.region.end, false)
                    }
                }
                .map_err(retained_error)?;
                if region.start < capability.practice.min_target
                    || region.end > capability.practice.max_target
                {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "retained region exceeds original bounds",
                    ));
                }
                None
            }
        };
        capability
            .rate
            .revision
            .checked_add(1)
            .ok_or(StepGameplayError::SequenceOverflow)?;
        let preparation = rate
            .map(|rate| {
                beatkernel::audio::prepare_practice_rate(
                    self.setup.config.program_id,
                    self.basis.source_hz(),
                    &self.setup.geometry,
                    rate,
                )
            })
            .transpose()
            .map_err(retained_error)?;
        if preparation.is_some_and(|preparation| {
            preparation.to_words()[6] > self.setup.config.limits.max_overlap as u64
                || preparation.to_words()[8] > self.setup.config.limits.max_events_per_render as u64
        }) {
            return Err(StepGameplayError::InvalidConfiguration(
                "rate exceeds retained overlap or event capacity",
            ));
        }
        let reservation = if matches!(request, PracticeTransactionRequest::Rate { .. }) {
            Some(
                self.business
                    .reserve_rate(request, transport, captures)
                    .map_err(retained_error)?,
            )
        } else {
            None
        };
        self.pending = Some(RetainedPending {
            request,
            region,
            rate: reservation,
            admitted: false,
        });
        Ok(StepRetainedControl {
            request,
            preparation,
        })
    }
    fn admission(
        &mut self,
        kind: StepRetainedAdmissionKind,
        id: u64,
        result: Result<(), String>,
    ) -> Result<(), StepGameplayError> {
        match kind {
            StepRetainedAdmissionKind::Transaction => {
                let pending =
                    self.pending
                        .as_mut()
                        .ok_or(StepGameplayError::InvalidConfiguration(
                            "no retained transaction awaits admission",
                        ))?;
                if transaction_identity(pending.request).0 != id || pending.admitted {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "retained admission identity differs",
                    ));
                }
                match result {
                    Ok(()) => pending.admitted = true,
                    Err(reason) => {
                        self.response = Some(match pending.request {
                            PracticeTransactionRequest::Region(request) => {
                                PracticeTransactionReply::Region {
                                    reply: PracticeReply {
                                        id: request.id,
                                        generation: request.generation,
                                        result: Err(reason),
                                    },
                                    rate: Some(self.business.rate_state()),
                                }
                            }
                            PracticeTransactionRequest::Rate { id, generation, .. } => {
                                PracticeTransactionReply::Rate {
                                    id,
                                    generation,
                                    result: Err(reason),
                                }
                            }
                            PracticeTransactionRequest::RestartOriginal {
                                id, generation, ..
                            } => PracticeTransactionReply::RestartOriginal {
                                id,
                                generation,
                                result: Err(reason),
                            },
                        });
                        self.pending = None;
                    }
                }
            }
            StepRetainedAdmissionKind::Pause => {
                let pause = self
                    .pause
                    .as_mut()
                    .ok_or(StepGameplayError::InvalidConfiguration(
                        "no retained pause awaits admission",
                    ))?;
                if pause.0 != id || pause.3 {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "retained pause admission differs",
                    ));
                }
                match result {
                    Ok(()) => pause.3 = true,
                    Err(reason) => {
                        self.pause = None;
                        return Err(retained_error(reason));
                    }
                }
            }
        }
        Ok(())
    }
    fn observe_history(
        &mut self,
        next: StepRemoteHistorySnapshot,
        authority: &AudioAuthority,
    ) -> Result<(), StepGameplayError> {
        let invalid = || {
            StepGameplayError::InvalidConfiguration(
                "retained source history changed or is malformed",
            )
        };
        if next.program_id != self.setup.config.program_id
            || next.source_hz != self.basis.source_hz()
            || next.initial_physical != 0
            || next.initial_playback != 0
            || next.retained_start > next.pulled_end
            || next.spans.len() > self.setup.config.source_history_capacity
            || next.spans.len() > 65_536
            || next.spans.is_empty() && (next.retained_start != 0 || next.pulled_end != 0)
            || next
                .spans
                .first()
                .is_some_and(|span| span.physical_start() != next.retained_start)
            || next.retained_start == 0
                && next
                    .spans
                    .first()
                    .is_some_and(|span| span.playback_start() != 0)
            || next.spans.last().is_some_and(|span| {
                span.physical_end().is_some() || span.physical_start() >= next.pulled_end
            })
        {
            return Err(invalid());
        }
        for spans in next.spans.windows(2) {
            let (left, right) = (spans[0], spans[1]);
            let playback = match left.state() {
                SourcePlaybackState::Frozen => Some(left.playback_start()),
                SourcePlaybackState::Running => right
                    .physical_start()
                    .checked_sub(left.physical_start())
                    .and_then(|delta| left.playback_start().checked_add(delta)),
            };
            if left.physical_end() != Some(right.physical_start())
                || playback != Some(right.playback_start())
            {
                return Err(invalid());
            }
        }
        if let Some(old) = &self.history {
            if next.context_start != old.context_start
                || next.pulled_end < old.pulled_end
                || next.retained_start < old.retained_start
                || next.retirement_revision < old.retirement_revision
                || (next.retained_start > old.retained_start)
                    != (next.retirement_revision > old.retirement_revision)
            {
                return Err(invalid());
            }
            let (mut old_index, mut new_index) = (0, 0);
            while old_index < old.spans.len() && new_index < next.spans.len() {
                let old_span = old.spans[old_index];
                let new_span = next.spans[new_index];
                let old_end = old_span.physical_end().unwrap_or(old.pulled_end);
                let new_end = new_span.physical_end().unwrap_or(next.pulled_end);
                let start = old_span
                    .physical_start()
                    .max(new_span.physical_start())
                    .max(next.retained_start);
                let end = old_end.min(new_end).min(old.pulled_end);
                if start < end {
                    let playback = |span: SourcePlaybackSpan| match span.state() {
                        SourcePlaybackState::Frozen => Some(span.playback_start()),
                        SourcePlaybackState::Running => span
                            .playback_start()
                            .checked_add(start - span.physical_start()),
                    };
                    if old_span.state() != new_span.state()
                        || playback(old_span) != playback(new_span)
                    {
                        return Err(invalid());
                    }
                }
                if old_end <= new_end {
                    old_index += 1;
                }
                if new_end <= old_end {
                    new_index += 1;
                }
            }
        }
        if next.retained_start > 0 {
            let pin = authority.future_input_retention_query()?;
            if pin
                .compare_source_time_frame(next.retained_start, next.source_hz)
                .map_err(retained_source_error)?
                .is_lt()
            {
                return Err(invalid());
            }
            if let Some((event, _)) = self.event {
                if self.event_frame(event) < next.retained_start {
                    return Err(invalid());
                }
            }
        }
        self.history = Some(next);
        Ok(())
    }
    fn event_frame(&self, event: WorkletRetainedEnvelope) -> u64 {
        match event.event {
            WorkletRetainedEvent::Pause(pause) => pause.physical_frame,
            WorkletRetainedEvent::Practice(projected) => projected.boundary.source_frame,
        }
    }
    fn retain_event(&mut self, event: WorkletRetainedEnvelope) -> Result<(), StepGameplayError> {
        let invalid = || {
            StepGameplayError::InvalidConfiguration(
                "retained event identity, order or payload differs",
            )
        };
        // Reuse the canonical codec's complete branch/reserved-field validation.
        if WorkletRetainedEnvelope::from_words(&event.to_words()).map_err(retained_error)? != event
            || event.program_id != self.setup.config.program_id
        {
            return Err(invalid());
        }
        if self.event.is_some_and(|(held, _)| held == event) || self.acknowledged == Some(event) {
            return Ok(());
        }
        if self.event.is_some()
            || event.serial
                != self
                    .acknowledged
                    .map_or(Some(1), |last| last.serial.checked_add(1))
                    .ok_or(StepGameplayError::SequenceOverflow)?
        {
            return Err(invalid());
        }
        if self
            .history
            .as_ref()
            .is_some_and(|history| history.context_start != event.context_start)
        {
            return Err(invalid());
        }
        self.event = Some((event, false));
        Ok(())
    }
    fn acknowledge(&mut self, serial: u64) -> Result<(), StepGameplayError> {
        match self.event {
            Some((event, true)) if event.serial == serial => {
                self.acknowledged = Some(event);
                self.event = None;
                Ok(())
            }
            None if self
                .acknowledged
                .is_some_and(|event| event.serial == serial) =>
            {
                Ok(())
            }
            _ => Err(StepGameplayError::InvalidConfiguration(
                "retained event serial differs or has not committed",
            )),
        }
    }
}

struct FreshStepMember {
    player: PlayerId,
    judge: JudgeEngine,
    gauge: BmsGauge,
    capture: Option<LiveReplayCapture>,
    header: PracticeRuntimeHeader,
}
#[allow(clippy::too_many_arguments)]
fn prepare_fresh_step_member(
    state: &StepRetained,
    index: usize,
    source: &beatkernel_bms::BmsChart,
    profile: beatkernel::judge::JudgeProfile,
    gauge: &crate::gauge::GaugeProfile,
    policy: Option<&crate::play_policy::ResolvedPlayPolicy>,
    input_mode: BmsInputMode,
    update: &RetainedBoundaryUpdate,
) -> Result<FreshStepMember, StepGameplayError> {
    let player = state.headers[index].player;
    let compiled = source.compile().map_err(retained_error)?;
    let judge = prepare_judge_with_timing(
        source,
        compiled.chart,
        profile,
        input_mode,
        beatkernel_bms::ParseOptions::default().max_objects,
        policy
            .and_then(|policy| policy.timing())
            .map(|timing| timing.profiles()),
    )
    .map_err(retained_error)?;
    if let Some(policy) = policy {
        policy
            .validate_timing(&judge, input_mode)
            .map_err(retained_error)?;
    }
    let sound_identity = InputSoundIdentity::from_source(source).map_err(retained_error)?;
    let speed = ReplaySpeed::new(update.rate.rate).map_err(retained_error)?;
    let limits = state.headers[index].limits;
    let header = if let Some(policy) = policy {
        crate::replay_capture::setup_play_policy_header_with_speed(
            &judge,
            state.basis.domain(),
            limits,
            update.region.start,
            state.options.chart_seed,
            update.end,
            input_mode,
            sound_identity,
            policy,
            speed,
        )
    } else {
        setup_gauge_header(
            &judge,
            state.basis.domain(),
            limits,
            update.region.start,
            state.options.chart_seed,
            update.end,
            input_mode,
            sound_identity,
            gauge,
        )
        .and_then(|header| {
            crate::replay_speed_policy::wrap_header(header, speed, limits)
                .map_err(CaptureError::SourceSpeed)
        })
    }
    .map_err(|error| StepGameplayError::Capture {
        error,
        report: None,
    })?;
    let capture = state.capture_limits[index]
        .map(|limits| {
            if let Some(policy) = policy {
                LiveReplayCapture::new_with_policy_and_speed(
                    &judge,
                    state.basis.domain(),
                    limits,
                    update.region.start,
                    state.options.chart_seed,
                    update.end,
                    input_mode,
                    sound_identity,
                    policy,
                    speed,
                )
            } else {
                LiveReplayCapture::new_with_gauge_and_speed(
                    &judge,
                    state.basis.domain(),
                    limits,
                    update.region.start,
                    state.options.chart_seed,
                    update.end,
                    input_mode,
                    sound_identity,
                    gauge,
                    speed,
                )
            }
        })
        .transpose()
        .map_err(|error| StepGameplayError::Capture {
            error,
            report: None,
        })?;
    if capture
        .as_ref()
        .is_some_and(|capture| capture.header() != &header)
    {
        return Err(StepGameplayError::InvalidConfiguration(
            "fresh retained capture differs from effective identity",
        ));
    }
    Ok(FreshStepMember {
        player,
        judge,
        gauge: BmsGauge::new(gauge.try_copy().map_err(retained_error)?),
        capture,
        header: PracticeRuntimeHeader {
            player,
            header,
            limits,
        },
    })
}

fn take_retired_capture(
    state: Option<&mut StepRetained>,
    player: PlayerId,
) -> Result<Option<Vec<u8>>, StepGameplayError> {
    let Some(state) = state else {
        return Ok(None);
    };
    let Some(index) = state.retired.iter().position(|(id, _)| *id == player) else {
        return Ok(None);
    };
    let (_, capture) = &state.retired[index];
    let bytes = capture
        .encode_bytes()
        .map_err(|error| StepGameplayError::Capture {
            error,
            report: None,
        })?;
    state.retired.remove(index);
    Ok(Some(bytes))
}

fn prepare_retained_setup(
    prepared: &PreparedRetainedSection,
    config: StepGameplayConfig,
    end: Option<Timestamp>,
    options: StepRetainedOptions,
    authority: &AudioAuthority,
) -> Result<StepRetainedSetup, StepGameplayError> {
    let selected =
        crate::section_start::source_at(&prepared.original_source, prepared.section.start)
            .map_err(retained_error)?
            .compile()
            .map_err(retained_error)?;
    let worklet = options.worklet;
    beatkernel::audio::PracticeLimits::new(
        worklet.limits.max_cues,
        worklet.limits.max_overlap,
        worklet.limits.max_events_per_render,
        worklet.limits.control_capacity,
        worklet.limits.receipt_capacity,
    )
    .map_err(retained_error)?;
    let hz = prepared.prepared.bank.format().sample_rate();
    let basis = OutputFrameBasis::new(config.output_origin, hz, 0).map_err(retained_error)?;
    if worklet.program_id == 0
        || worklet.initial_region.start != prepared.section.start
        || worklet.initial_region.start.as_nanos() < 0
        || worklet.initial_region.start >= worklet.initial_region.end
        || worklet.output_preroll != config.preroll
        || worklet.initial_rate.numerator() <= 0
        || worklet.open_end != end.is_none()
        || end.is_some_and(|end| end != worklet.initial_region.end)
        || !(1..=65_536).contains(&worklet.source_history_capacity)
        || !matches!(authority.coordinate_basis(), Some(AudioAuthorityCoordinateBasis::SourceFrame(actual)) if actual == basis)
            && !matches!(authority.coordinate_basis(), Some(AudioAuthorityCoordinateBasis::SourceCounter { basis: actual, frequency }) if actual == basis && frequency > 0)
            && !matches!(authority.coordinate_basis(), Some(AudioAuthorityCoordinateBasis::SourceTimestamp(actual)) if actual == basis)
        || selected.chart.objects() != prepared.prepared.compiled.chart.objects()
    {
        return Err(StepGameplayError::InvalidConfiguration(
            "retained setup differs from original section, PCM or authority basis",
        ));
    }
    let cues =
        crate::bgm::retained_bgm_cues(&prepared.prepared.bgm_commands).map_err(retained_error)?;
    if cues.len() > worklet.limits.max_cues {
        return Err(StepGameplayError::InvalidConfiguration(
            "retained cues exceed configured capacity",
        ));
    }
    let geometry = crate::bgm::retained_rate_geometry(&prepared.prepared.bank, &cues)
        .map_err(retained_error)?;
    let initial = beatkernel::audio::prepare_practice_rate(
        worklet.program_id,
        hz,
        &geometry,
        worklet.initial_rate,
    )
    .map_err(retained_error)?
    .to_words();
    if initial[6] > worklet.limits.max_overlap as u64
        || initial[8] > worklet.limits.max_events_per_render as u64
    {
        return Err(StepGameplayError::InvalidConfiguration(
            "initial retained geometry exceeds capacity",
        ));
    }
    Ok(StepRetainedSetup {
        config: worklet,
        cues,
        geometry,
        sample_count: u32::try_from(prepared.prepared.bank.len())
            .map_err(|_| StepGameplayError::SequenceOverflow)?,
    })
}

fn prepare_retained_completion(
    prepared: &mut PreparedRetainedSection,
    config: StepGameplayConfig,
    policy: Option<&crate::play_policy::ResolvedPlayPolicy>,
) -> Result<SongCompletion, StepGameplayError> {
    let full_compiled = prepared.original_source.compile().map_err(retained_error)?;
    let selected_source = std::mem::replace(
        &mut prepared.prepared.source,
        prepared.original_source.clone(),
    );
    let selected_compiled = std::mem::replace(&mut prepared.prepared.compiled, full_compiled);
    let result = SongCompletion::prepare(
        &prepared.prepared,
        policy.map_or(config.late_ns, |policy| policy.completion_late().as_nanos()),
        config.offset_ns,
        config.preroll.as_nanos(),
        config.output_origin.domain,
    )
    .map_err(retained_error);
    prepared.prepared.source = selected_source;
    prepared.prepared.compiled = selected_compiled;
    result
}

fn retained_event_query(
    event: WorkletRetainedEnvelope,
    hz: u32,
) -> Result<InterpolatedTargetTime, StepGameplayError> {
    let (frame, projected) = match event.event {
        WorkletRetainedEvent::Pause(pause) => (pause.physical_frame, None),
        WorkletRetainedEvent::Practice(projected) => {
            (projected.boundary.source_frame, Some(projected))
        }
    };
    let time = TargetTime::from_frames(frame, hz).map_err(retained_error)?;
    if projected.is_some_and(|projected| {
        projected.target_rate != hz
            || projected.target_frame != frame
            || projected.boundary.target_time != time
            || projected.origin != crate::worklet_audio::WORKLET_MIXER_ORIGIN
    }) {
        return Err(StepGameplayError::InvalidConfiguration(
            "retained Worklet projection differs from original source grid",
        ));
    }
    InterpolatedTargetTime::new(time, 0, 0, 1, 0, 1).map_err(retained_error)
}

struct RetainedBoundaryUpdate {
    receipt: beatkernel::audio::PracticeReceipt,
    region: PracticeRegion,
    rate: PracticeRateState,
    fresh: bool,
    restart: bool,
    end: Option<Timestamp>,
    response: Option<PracticeTransactionReply>,
}
impl StepRetained {
    fn boundary_update(
        &self,
        event: PracticeEvent,
        end: Option<Timestamp>,
    ) -> Result<RetainedBoundaryUpdate, StepGameplayError> {
        let invalid = || {
            StepGameplayError::InvalidConfiguration(
                "retained boundary differs from actual attempt or request",
            )
        };
        let (receipt, restart) = match event {
            PracticeEvent::Boundary(receipt) => (receipt, None),
            PracticeEvent::Restart(restart) => (restart.boundary, Some(restart)),
            PracticeEvent::Rate(_) => return Err(invalid()),
        };
        let mut region = self.region;
        let mut rate = self.business.rate_state();
        let mut fresh = false;
        let mut new_end = end;
        let mut response = None;
        if let Some(restart) = restart {
            let pending = self
                .pending
                .as_ref()
                .filter(|pending| pending.admitted)
                .ok_or_else(invalid)?;
            let PracticeTransactionRequest::RestartOriginal {
                id,
                generation,
                expected,
            } = pending.request
            else {
                return Err(invalid());
            };
            if receipt.request_id != id
                || restart.request_revision != expected.revision
                || restart.previous_rate != rate.rate
                || restart.requested_rate != self.setup.config.initial_rate
            {
                return Err(invalid());
            }
            match restart.outcome {
                PracticeRateOutcome::Applied => {
                    if generation != self.generation
                        || expected != rate
                        || receipt.kind != PracticeBoundaryKind::Requested
                        || self.generation.checked_add(1) != Some(receipt.generation)
                        || receipt.iteration != 0
                        || restart.applied_rate != self.setup.config.initial_rate
                        || expected.revision.checked_add(1) != Some(restart.applied_revision)
                    {
                        return Err(invalid());
                    }
                    region = self.setup.config.initial_region;
                    new_end = if self.setup.config.open_end {
                        None
                    } else {
                        Some(region.end)
                    };
                    rate = PracticeRateState {
                        revision: restart.applied_revision,
                        rate: restart.applied_rate,
                    };
                    fresh = true;
                }
                PracticeRateOutcome::Refused(reason) => {
                    if receipt.kind != PracticeBoundaryKind::ControlRejectedRevision
                        || receipt.generation != self.generation
                        || receipt.iteration != self.iteration
                        || restart.applied_revision != rate.revision
                        || restart.applied_rate != rate.rate
                        || generation != self.generation
                            && reason != beatkernel::audio::PracticeRateRefusal::StaleGeneration
                    {
                        return Err(invalid());
                    }
                }
                PracticeRateOutcome::Noop => return Err(invalid()),
            }
            response = Some(PracticeTransactionReply::RestartOriginal {
                id,
                generation,
                result: Ok(restart),
            });
        } else {
            match receipt.kind {
                PracticeBoundaryKind::Started => {
                    if self.started
                        || receipt.request_id != 0
                        || receipt.generation != 1
                        || receipt.iteration != 0
                        || receipt.physical_frame != 0
                        || receipt.playback_frame != 0
                        || receipt.requested_song_time != region.start
                        || receipt.applied_song_time
                            != self.phase.song_at_elapsed(0).map_err(retained_error)?
                    {
                        return Err(invalid());
                    }
                }
                PracticeBoundaryKind::Requested
                | PracticeBoundaryKind::LoopDisabled
                | PracticeBoundaryKind::ControlRejectedRevision => {
                    let pending = self
                        .pending
                        .as_ref()
                        .filter(|pending| pending.admitted)
                        .ok_or_else(invalid)?;
                    let PracticeTransactionRequest::Region(request) = pending.request else {
                        return Err(invalid());
                    };
                    if !self.started
                        || receipt.request_id != request.id
                        || request.generation > self.generation
                    {
                        return Err(invalid());
                    }
                    if receipt.kind == PracticeBoundaryKind::Requested {
                        if request.action == PracticeAction::DisableLoop
                            || receipt.generation
                                != self
                                    .generation
                                    .checked_add(1)
                                    .ok_or(StepGameplayError::SequenceOverflow)?
                            || receipt.iteration != 0
                        {
                            return Err(invalid());
                        }
                        region = pending.region;
                        new_end = match request.action {
                            PracticeAction::Scrub { .. } if self.setup.config.open_end => None,
                            _ => Some(region.end),
                        };
                        fresh = true;
                        rate.revision = rate
                            .revision
                            .checked_add(1)
                            .ok_or(StepGameplayError::SequenceOverflow)?;
                    } else {
                        if receipt.generation != self.generation
                            || receipt.iteration != self.iteration
                        {
                            return Err(invalid());
                        }
                        if receipt.kind == PracticeBoundaryKind::LoopDisabled {
                            if request.action != PracticeAction::DisableLoop {
                                return Err(invalid());
                            }
                            region.repeat = false;
                            rate.revision = rate
                                .revision
                                .checked_add(1)
                                .ok_or(StepGameplayError::SequenceOverflow)?;
                        }
                    }
                    response = Some(PracticeTransactionReply::Region {
                        reply: PracticeReply {
                            id: request.id,
                            generation: request.generation,
                            result: if receipt.kind == PracticeBoundaryKind::ControlRejectedRevision
                            {
                                Err("actual output refused stale region revision".into())
                            } else {
                                Ok(PracticeApplied {
                                    generation: receipt.generation,
                                    physical_frame: receipt.physical_frame,
                                    playback_frame: receipt.playback_frame,
                                    requested_target: receipt.requested_song_time,
                                    applied_target: receipt.applied_song_time,
                                })
                            },
                        },
                        rate: Some(rate),
                    });
                }
                PracticeBoundaryKind::Looped | PracticeBoundaryKind::Ended => {
                    if !self.started
                        || self.ended
                        || receipt.request_id != 0
                        || self.generation.checked_add(1) != Some(receipt.generation)
                        || self.iteration.checked_add(1) != Some(receipt.iteration)
                        || (receipt.kind == PracticeBoundaryKind::Looped) != region.repeat
                    {
                        return Err(invalid());
                    }
                    fresh = receipt.kind == PracticeBoundaryKind::Looped;
                }
            }
        }
        if fresh {
            if receipt.requested_song_time != region.start
                || receipt.applied_song_time != region.start
                || receipt.correction_nanos != 0
            {
                return Err(invalid());
            }
        } else if receipt.kind == PracticeBoundaryKind::Started {
            if i128::from(receipt.applied_song_time.as_nanos())
                - i128::from(receipt.requested_song_time.as_nanos())
                != i128::from(receipt.correction_nanos)
            {
                return Err(invalid());
            }
        } else if receipt.kind == PracticeBoundaryKind::Ended {
            if receipt.requested_song_time != region.end
                || receipt.applied_song_time != region.end
                || receipt.correction_nanos != 0
            {
                return Err(invalid());
            }
        } else if receipt.kind != PracticeBoundaryKind::ControlRejectedRevision
            && (receipt.requested_song_time != receipt.applied_song_time
                || receipt.correction_nanos != 0)
        {
            return Err(invalid());
        }
        Ok(RetainedBoundaryUpdate {
            receipt,
            region,
            rate,
            fresh,
            restart: restart.is_some_and(|event| event.outcome == PracticeRateOutcome::Applied),
            end: new_end,
            response,
        })
    }
}

/// Explicit mapping and bounded control-side resources for a fresh solo run.
#[derive(Clone, Copy, Debug)]
pub struct StepGameplayConfig {
    /// Host instant at which song position is section start minus preroll.
    pub host_origin: ClockPoint,
    /// Frame zero of the separate output grid; browser mixers use timestamp zero.
    pub output_origin: ClockPoint,
    pub preroll: Duration,
    pub early_ns: i64,
    pub late_ns: i64,
    pub offset_ns: i64,
    pub command_capacity: usize,
    /// Positive BGM credit strictly below command_capacity, reserving input space.
    pub bgm_pending: usize,
    pub bgm_lookahead: Duration,
    pub telemetry_capacity: usize,
}

/// One immutable admission attempt. A failed prefix must never be retried.
#[derive(Clone, Debug, PartialEq)]
pub struct StepAudioBatch {
    pub sequence: u64,
    pub commands: Vec<AudioCommand>,
}

/// Actual failure evidence is retained independently from the owner's fence.
#[derive(Debug)]
pub enum StepGameplayError {
    /// Public bounds failed before gameplay or queue mutation.
    InvalidConfiguration(&'static str),
    Setup(String),
    Failed,
    /// Snapshot extraction alone is busy; judging and BGM admission may continue.
    OutstandingBatch {
        sequence: u64,
    },
    /// Batch buffers could not be reserved; no commands were consumed.
    AllocationFailed,
    SequenceOverflow,
    Runtime(GroupError),
    /// Original clock discipline failure; committed judgments remain retained.
    Clock(DisciplineError),
    /// Audio correspondence failure, distinct from optional legacy rate discipline.
    AudioAuthority(AudioAuthorityError),
    /// The report's judgments remain committed even when audio admission failed.
    /// Score aggregation is atomic; its error preserves the previous score.
    Report {
        report: RuntimeReport,
        score_error: Option<CompetitionError>,
        capture_error: Option<CaptureError>,
    },
    /// Mine aggregation refused an already committed report. Other processing
    /// errors remain attached, and the previous damage summary is unchanged.
    MineDamage {
        error: MineDamageError,
        report: RuntimeReport,
        score_error: Option<CompetitionError>,
        capture_error: Option<CaptureError>,
    },
    /// Gauge aggregation refused a committed report without losing independent
    /// postprocessing errors or changing the previous atomic gauge snapshot.
    Gauge {
        error: GaugeError,
        report: RuntimeReport,
        score_error: Option<CompetitionError>,
        capture_error: Option<CaptureError>,
        mine_error: Option<MineDamageError>,
    },
    /// Capture rejected a committed operation, or setup/export failed without
    /// an operation. Earlier recorded operations remain the accepted prefix.
    Capture {
        error: CaptureError,
        report: Option<RuntimeReport>,
    },
    Bgm {
        error: BgmFeedError,
        report: BgmFeedReport,
    },
    /// Original output evidence is retained; invalid evidence fences the owner.
    Completion {
        error: CompletionError,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    },
    Queue {
        error: QueuePopError,
        commands: Vec<AudioCommand>,
    },
    /// A popped command could not be mapped; its earlier eligible prefix remains exact.
    CommandMapping {
        error: ReplayAudioError,
        command: AudioCommand,
        commands: Vec<AudioCommand>,
    },
    /// The original full-success ACK was valid, but its cumulative count overflowed.
    AudioCountOverflow {
        sequence: u64,
        admitted: usize,
    },
    /// A valid remote rejection preserves the exact batch and admitted prefix.
    AudioRejected {
        batch: StepAudioBatch,
        admitted: usize,
    },
    /// Received fields are untrusted; admitted is not validated prefix evidence.
    InvalidAcknowledgement {
        sequence: u64,
        admitted: usize,
        success: bool,
        batch: Option<StepAudioBatch>,
    },
}

impl fmt::Display for StepGameplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(f, "step gameplay configuration: {message}")
            }
            Self::Setup(message) => write!(f, "step gameplay setup: {message}"),
            Self::Failed => f.write_str("step gameplay is fenced"),
            Self::OutstandingBatch { sequence } => {
                write!(f, "audio batch {sequence} awaits acknowledgement")
            }
            Self::AllocationFailed => {
                f.write_str("audio batch allocation failed before queue consumption")
            }
            Self::SequenceOverflow => f.write_str("audio batch sequence exhausted"),
            Self::Runtime(error) => write!(f, "step gameplay: {error}"),
            Self::Clock(error) => write!(f, "step gameplay: {error}"),
            Self::AudioAuthority(error) => write!(f, "step gameplay: {error}"),
            Self::Report {
                report,
                score_error,
                capture_error,
            } => write!(
                f,
                "step gameplay report failed: judge {:?}, {} audio failures, score {:?}, capture {:?}",
                report.judge_error,
                report.audio_failures.len(),
                score_error,
                capture_error
            ),
            Self::MineDamage {
                error,
                report,
                score_error,
                capture_error,
            } => write!(
                f,
                "step gameplay mine damage: {error}; judge {:?}, {} audio failures, score {:?}, capture {:?}",
                report.judge_error,
                report.audio_failures.len(),
                score_error,
                capture_error
            ),
            Self::Gauge {
                error,
                report,
                score_error,
                capture_error,
                mine_error,
            } => write!(
                f,
                "step gameplay gauge: {error}; judge {:?}, {} audio failures, score {:?}, capture {:?}, mine damage {:?}",
                report.judge_error,
                report.audio_failures.len(),
                score_error,
                capture_error,
                mine_error
            ),
            Self::Capture { error, .. } => write!(f, "step gameplay capture: {error}"),
            Self::Bgm { error, report } => write!(
                f,
                "{error}; {} BGM admissions remain committed",
                report.total_admitted
            ),
            Self::Completion { error, .. } => write!(f, "step gameplay completion: {error}"),
            Self::Queue { error, commands } => write!(
                f,
                "audio queue {error:?} after {} consumed commands",
                commands.len()
            ),
            Self::CommandMapping {
                error,
                command,
                commands,
            } => write!(
                f,
                "audio command {command:?} mapping failed after {} eligible commands: {error}",
                commands.len()
            ),
            Self::AudioCountOverflow { sequence, admitted } => write!(
                f,
                "audio acknowledgement {sequence} committed {admitted} commands beyond the count range"
            ),
            Self::AudioRejected { batch, admitted } => write!(
                f,
                "audio batch {} rejected after {admitted}/{} commands",
                batch.sequence,
                batch.commands.len()
            ),
            Self::InvalidAcknowledgement {
                sequence,
                admitted,
                success,
                batch,
            } => write!(
                f,
                "invalid audio acknowledgement {sequence} admitted {admitted} success {success}, expected {:?}",
                batch.as_ref().map(|batch| batch.sequence)
            ),
        }
    }
}
impl Error for StepGameplayError {}
impl From<AudioAuthorityError> for StepGameplayError {
    fn from(error: AudioAuthorityError) -> Self {
        Self::AudioAuthority(error)
    }
}

// Logical progression and raw scheduling points already belong to their declared
// Runtime domains; no cross-domain conversion is supplied for deadline advancement.
struct AudioDomains;
impl ClockMapper for AudioDomains {
    fn map(&self, _: ClockPoint, _: ClockDomainId) -> Option<Timestamp> {
        None
    }
    fn quality(&self) -> beatkernel::time::ClockMappingQuality {
        beatkernel::time::ClockMappingQuality::Unknown
    }
}

enum InputSetup {
    Solo(BindingMap),
    Local(ResolvedInputPlan, Vec<BindingMap>),
}

enum RuntimeSetup {
    Solo {
        bindings: BindingMap,
        judge: JudgeEngine,
        input_sounds: Option<InputSoundTimeline>,
        hazard_sounds: Option<HazardSoundTimeline>,
    },
    Local {
        members: PreparedLocalMembers,
        primary: PlayerId,
        input_sounds: Vec<(PlayerId, InputSoundTimeline)>,
        hazard_sounds: Vec<(PlayerId, HazardSoundTimeline)>,
    },
}

/// Both paths retain one authoritative transport and producer. Scalar gameplay
/// is admitted only for Solo; Local operations are owned by StepLocalGameplay.
enum RuntimeOwner {
    Solo(SoloRuntime),
    Local {
        group: RuntimeGroup,
        primary: PlayerId,
    },
}

impl RuntimeOwner {
    fn solo_mut(&mut self) -> Result<&mut SoloRuntime, StepGameplayError> {
        match self {
            Self::Solo(runtime) => Ok(runtime),
            Self::Local { .. } => Err(StepGameplayError::InvalidConfiguration(
                "scalar gameplay cannot operate a local cohort",
            )),
        }
    }

    fn judge(&self) -> &JudgeEngine {
        match self {
            Self::Solo(runtime) => runtime.judge(),
            Self::Local { group, primary } => group
                .member_judge(*primary)
                .expect("prepared primary belongs to the private cohort"),
        }
    }

    fn transport_mut(&mut self) -> &mut Transport {
        match self {
            Self::Solo(runtime) => runtime.transport_mut(),
            Self::Local { group, .. } => group.transport_mut(),
        }
    }

    fn seal_gameplay_audio(&mut self) {
        match self {
            Self::Solo(runtime) => runtime.seal_gameplay_audio(),
            Self::Local { group, .. } => group.seal_gameplay_audio(),
        }
    }

    fn enqueue_audio(&mut self, command: AudioCommand) -> Result<(), CommandPushError> {
        match self {
            Self::Solo(runtime) => runtime.enqueue_audio(command),
            Self::Local { group, .. } => group.enqueue_audio(command),
        }
    }

    fn set_processing_clock(&mut self, clock: RuntimeProcessingClock) {
        match self {
            Self::Solo(runtime) => runtime.set_processing_clock(clock),
            Self::Local { group, .. } => group.set_processing_clock(clock),
        }
    }

    fn set_song_end(&mut self, end: Timestamp) -> Result<(), String> {
        match self {
            Self::Solo(runtime) => runtime.set_song_end(end),
            Self::Local { group, .. } => group.set_song_end(end),
        }
    }
}

/// Sole runtime producer and queue consumer, outside every audio callback.
///
/// The caller owns the returned PCM bank and the remote output lifecycle. This
/// owner never treats a command ACK, a UI tick or failure as playback completion.
pub struct StepGameplay {
    runtime: RuntimeOwner,
    consumer: CommandConsumer,
    bgm: BgmFeeder,
    completion: Option<SongCompletion>,
    output_origin: ClockPoint,
    sample_rate: u32,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    output_clock: Option<PresentationDiscipline>,
    authority: Option<AudioAuthority>,
    correction_watermark: Option<ClockPoint>,
    capture: Option<LiveReplayCapture>,
    capture_configured: bool,
    score: ScoreSummary,
    mine_damage: MineDamageSummary,
    gauge: BmsGauge,
    policy: Option<crate::play_policy::ResolvedPlayPolicy>,
    completed_result: Option<CompletedPlayResult>,
    song: Timestamp,
    host_domain: ClockDomainId,
    logical_domain: ClockDomainId,
    start: Timestamp,
    end: Option<Timestamp>,
    input_mode: BmsInputMode,
    input_sound_identity: Option<InputSoundIdentity>,
    playback_end_frame: Option<u64>,
    preroll: Duration,
    live_speed: Option<LiveSpeedSetup>,
    last_operation: Option<ClockPoint>,
    pre_origin_inputs: u64,
    live_terminal: bool,
    activated: bool,
    started: bool,
    pending: Option<StepAudioBatch>,
    sequence: u64,
    acknowledged_commands: u64,
    acknowledged_stops: OwnedStopEvidence,
    failed: bool,
    retained: Option<StepRetained>,
}

impl fmt::Debug for StepGameplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepGameplay")
            .field("song", &self.song)
            .field("score", &self.score)
            .field("mine_damage", &self.mine_damage)
            .field("gauge", &self.gauge)
            .field("completed_result", &self.completed_result)
            .field(
                "pending_sequence",
                &self.pending.as_ref().map(|batch| batch.sequence),
            )
            .field("failed", &self.failed)
            .field("activated", &self.activated)
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl StepGameplay {
    #[allow(clippy::too_many_arguments)]
    pub fn new_retained_audio_section(
        mut prepared: PreparedRetainedSection,
        config: StepGameplayConfig,
        bindings: BindingMap,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        policy: Option<crate::play_policy::ResolvedPlayPolicy>,
        retained: StepRetainedOptions,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        let setup = prepare_retained_setup(&prepared, config, end, retained, &authority)?;
        let completion = prepare_retained_completion(&mut prepared, config, policy.as_ref())?;
        let start = prepared.section.start;
        let (mut owner, bank) = Self::build_mode(
            prepared.prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            Some(authority),
            policy,
            None,
            true,
        )?;
        owner.live_speed = Some(
            LiveSpeedSetup::new(
                start,
                config.preroll,
                ReplaySpeed::new(retained.worklet.initial_rate).map_err(retained_error)?,
                end,
            )
            .map_err(retained_error)?,
        );
        let headers = vec![PracticeRuntimeHeader {
            player: PlayerId(1),
            header: owner.recording_header(retained.identity_limits, retained.chart_seed)?,
            limits: retained.identity_limits,
        }];
        owner.install_retained(
            prepared.original_source,
            retained,
            setup,
            headers,
            completion,
        )?;
        Ok((owner, bank))
    }
    fn install_retained(
        &mut self,
        original: beatkernel_bms::BmsChart,
        options: StepRetainedOptions,
        setup: StepRetainedSetup,
        headers: Vec<PracticeRuntimeHeader>,
        completion_template: SongCompletion,
    ) -> Result<(), StepGameplayError> {
        let basis = OutputPhaseBasis::new(
            self.logical_domain,
            options.worklet.program_id,
            1,
            0,
            self.sample_rate,
        )
        .map_err(retained_error)?;
        let logical = self
            .authority
            .as_ref()
            .expect("retained construction requires authority")
            .epoch()
            .logical_origin;
        let phase = PositiveRatePhase::new(
            self.start,
            self.preroll,
            options.worklet.initial_rate,
            self.end,
        )
        .map_err(retained_error)?;
        let transport = Transport::new_with_runtime_output_phase(
            basis,
            logical,
            self.start,
            self.preroll,
            options.worklet.initial_rate,
            self.end,
        )
        .map_err(retained_error)?;
        let business = RuntimePractice::new(headers.clone(), &[], phase).map_err(retained_error)?;
        let source =
            crate::section_start::source_at(&original, self.start).map_err(retained_error)?;
        let completion = completion_template
            .prepare_practice(&source, self.runtime.judge())
            .map_err(retained_error)?;
        let mut retired = Vec::new();
        retired
            .try_reserve_exact(headers.len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        *self.runtime.transport_mut() = transport;
        match &mut self.runtime {
            RuntimeOwner::Solo(runtime) => runtime.set_audio_scope(CommandScope(1)),
            RuntimeOwner::Local { group, .. } => group.set_audio_scope(CommandScope(1)),
        }
        self.song = phase.song_at_elapsed(0).map_err(retained_error)?;
        self.completion = Some(completion);
        let capture_limits = vec![None; headers.len()];
        self.retained = Some(StepRetained {
            setup,
            original,
            options,
            business,
            phase,
            headers,
            capture_limits,
            completion_template,
            basis,
            region: options.worklet.initial_region,
            generation: 1,
            iteration: 0,
            started: false,
            ended: false,
            paused: false,
            pending: None,
            pause: None,
            history: None,
            event: None,
            acknowledged: None,
            response: None,
            retired,
        });
        Ok(())
    }
    pub fn retained_setup(&self) -> Option<&StepRetainedSetup> {
        self.retained.as_ref().map(|state| &state.setup)
    }
    pub fn configure_retained_limits(
        &mut self,
        limits: beatkernel::audio::PracticeLimits,
        source_history_capacity: usize,
    ) -> Result<(), StepGameplayError> {
        self.configure_retained_limits_inner(limits, source_history_capacity, None)
    }
    /// Reserves actual initial BGM overlap; all remaining Mixer slots stay shared by gameplay.
    pub fn configure_retained_audio_limits(
        &mut self,
        total_voices: usize,
        max_events_per_render: usize,
        control_capacity: usize,
        receipt_capacity: usize,
        source_history_capacity: usize,
    ) -> Result<(), StepGameplayError> {
        let max_cues = self
            .retained_setup()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?
            .cues
            .len()
            .max(1);
        self.configure_retained_limits_inner(
            beatkernel::audio::PracticeLimits {
                max_cues,
                max_overlap: 0,
                max_events_per_render,
                control_capacity,
                receipt_capacity,
            },
            source_history_capacity,
            Some(total_voices),
        )
    }
    fn configure_retained_limits_inner(
        &mut self,
        mut limits: beatkernel::audio::PracticeLimits,
        source_history_capacity: usize,
        total_voices: Option<usize>,
    ) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        let state = self
            .retained
            .as_mut()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?;
        if self.activated
            || self.started
            || self.capture_configured
            || state.capture_limits.iter().any(Option::is_some)
            || state.started
            || state.pending.is_some()
            || state.pause.is_some()
            || state.event.is_some()
            || state.history.is_some()
            || self.authority.as_ref().is_some_and(|authority| {
                authority.latest_observation().is_some() || authority.acquired_prefix().is_some()
            })
            || !(1..=65_536).contains(&source_history_capacity)
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained limits require pristine cold setup",
            ));
        }
        beatkernel::audio::PracticeLimits::new(
            limits.max_cues,
            limits.max_overlap,
            limits.max_events_per_render,
            limits.control_capacity,
            limits.receipt_capacity,
        )
        .map_err(retained_error)?;
        if total_voices.is_some_and(|total| {
            !(1..=AudioLimits::MAX_VOICES).contains(&total)
                || limits.max_events_per_render > AudioLimits::MAX_COMMANDS
        }) {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained audio limits exceed total Mixer capacity",
            ));
        }
        let preparation = beatkernel::audio::prepare_practice_rate(
            state.setup.config.program_id,
            self.sample_rate,
            &state.setup.geometry,
            state.setup.config.initial_rate,
        )
        .map_err(retained_error)?
        .to_words();
        if let Some(total) = total_voices {
            limits.max_overlap =
                usize::try_from(preparation[6]).map_err(|_| StepGameplayError::SequenceOverflow)?;
            let gameplay = total.checked_sub(limits.max_overlap).ok_or(
                StepGameplayError::InvalidConfiguration(
                    "retained BGM overlap exceeds total voice budget",
                ),
            )?;
            if gameplay == 0
                && (!state.original.notes.is_empty()
                    || !state.original.invisible.is_empty()
                    || !state.original.mines.is_empty())
            {
                return Err(StepGameplayError::InvalidConfiguration(
                    "retained total voice budget leaves no gameplay voice",
                ));
            }
        }
        if state.setup.cues.len() > limits.max_cues
            || preparation[6] > limits.max_overlap as u64
            || preparation[8] > limits.max_events_per_render as u64
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained initial geometry exceeds agreed audio capacity",
            ));
        }
        state.setup.config.limits = limits;
        state.setup.config.source_history_capacity = source_history_capacity;
        state.options.worklet.limits = limits;
        state.options.worklet.source_history_capacity = source_history_capacity;
        Ok(())
    }
    pub fn retained_capability(&self) -> Option<PracticeTransactionCapability> {
        if self.failed {
            None
        } else {
            self.retained.as_ref().and_then(StepRetained::capability)
        }
    }
    pub fn effective_header(&self, player: PlayerId) -> Option<&ReplayHeader> {
        self.retained.as_ref()?.business.effective_header(player)
    }
    pub fn prepare_retained_transaction(
        &mut self,
        request: PracticeTransactionRequest,
        fixed_competition: bool,
    ) -> Result<StepRetainedControl, StepGameplayError> {
        self.ensure_usable()?;
        self.ensure_solo()?;
        let state = self
            .retained
            .as_mut()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?;
        state.prepare(
            request,
            fixed_competition,
            self.runtime.transport_mut(),
            &mut [(PlayerId(1), &mut self.capture)],
        )
    }
    pub fn prepare_retained_pause(
        &mut self,
        id: u64,
        generation: u64,
        paused: bool,
    ) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        let state = self
            .retained
            .as_mut()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?;
        if id == 0
            || !state.started
            || state.ended
            || generation != state.generation
            || state.pause.is_some()
            || state.event.is_some()
            || state.pending.is_some() && (paused || !state.paused)
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained pause is busy or stale",
            ));
        }
        self.runtime
            .transport_mut()
            .try_reserve_output_phase_controls(1)
            .map_err(retained_error)?;
        state.pause = Some((id, generation, paused, false));
        Ok(())
    }
    pub fn acknowledge_retained_admission(
        &mut self,
        kind: StepRetainedAdmissionKind,
        id: u64,
        result: Result<(), String>,
    ) -> Result<(), StepGameplayError> {
        self.retained
            .as_mut()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?
            .admission(kind, id, result)
    }
    pub fn observe_retained_history(
        &mut self,
        snapshot: StepRemoteHistorySnapshot,
    ) -> Result<(), StepGameplayError> {
        let authority = self
            .authority
            .as_ref()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "audio authority is unavailable",
            ))?;
        self.retained
            .as_mut()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?
            .observe_history(snapshot, authority)
    }
    pub fn retain_output_event(
        &mut self,
        event: WorkletRetainedEnvelope,
    ) -> Result<(), StepGameplayError> {
        self.retained
            .as_mut()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?
            .retain_event(event)
    }
    pub fn take_retained_response(&mut self) -> Option<PracticeTransactionReply> {
        self.retained.as_mut()?.response.take()
    }
    pub fn acknowledge_retained_event(&mut self, serial: u64) -> Result<(), StepGameplayError> {
        self.retained
            .as_mut()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?
            .acknowledge(serial)
    }
    pub fn retained_history_pin(&self) -> Result<Option<u64>, StepGameplayError> {
        let Some(state) = &self.retained else {
            return Ok(None);
        };
        let Some(history) = &state.history else {
            return Ok(Some(0));
        };
        let query = self
            .authority
            .as_ref()
            .expect("retained authority")
            .future_input_retention_query()?;
        let mut low = 0u64;
        let mut high = history.pulled_end;
        while low < high {
            let mid = low + (high - low) / 2 + 1;
            if query
                .compare_source_time_frame(mid, state.basis.source_hz())
                .map_err(retained_source_error)?
                .is_lt()
            {
                high = mid - 1;
            } else {
                low = mid;
            }
        }
        if let Some((event, _)) = state.event {
            low = low.min(state.event_frame(event));
        }
        Ok(Some(low))
    }
    pub fn take_retired_replay(
        &mut self,
        player: PlayerId,
    ) -> Result<Option<Vec<u8>>, StepGameplayError> {
        self.ensure_solo()?;
        if player != PlayerId(1) {
            return Err(StepGameplayError::InvalidConfiguration(
                "unknown solo player",
            ));
        }
        take_retired_capture(self.retained.as_mut(), player)
    }
    pub fn observe_audio_coordinate(
        &mut self,
        epoch: u64,
        pair: ClockPair,
        coordinate: crate::native_output_coordinate::NativeOutputCoordinate,
    ) -> Result<ObservationAdmission, StepGameplayError> {
        self.ensure_audio()?;
        self.authority
            .as_mut()
            .expect("audio mode checked")
            .observe_coordinate(epoch, pair, coordinate)
            .map_err(Into::into)
    }
    pub fn observe_audio_source_timestamp(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, StepGameplayError> {
        self.ensure_audio()?;
        self.authority
            .as_mut()
            .expect("audio mode checked")
            .observe_source_timestamp(epoch, pair)
            .map_err(Into::into)
    }
    fn retained_input(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<crate::audio_authority::PreparedCoordinateInput>, StepGameplayError> {
        self.validate_audio_operation(now, audio_at, None)?;
        loop {
            let state = self.retained.as_ref().expect("retained branch");
            let authority = self.authority.as_mut().expect("audio mode checked");
            let Some(prefix) = authority.acquired_prefix() else {
                return Ok(None);
            };
            let Some(event) = merger
                .peek_ready(prefix)
                .map_err(AudioAuthorityError::from)?
            else {
                return Ok(None);
            };
            let Some(prepared) = authority.prepare_coordinate_input(
                ClockPoint {
                    domain: event.meta().clock_domain,
                    timestamp: event.meta().timestamp,
                },
                now,
            )?
            else {
                return Ok(None);
            };
            if prepared.is_before_origin() {
                let count = self
                    .pre_origin_inputs
                    .checked_add(1)
                    .ok_or(StepGameplayError::SequenceOverflow)?;
                authority.commit_discarded_coordinate_input(prepared, now)?;
                merger
                    .pop_ready(prefix)
                    .map_err(AudioAuthorityError::from)?;
                self.pre_origin_inputs = count;
                continue;
            }
            if !state.started {
                return Ok(None);
            }
            if !state.permits_query(prepared.query())? {
                return Ok(None);
            }
            state.phase_point(prepared.output(), prepared.query())?;
            return Ok(Some(prepared));
        }
    }
    fn process_next_retained_input(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
        position: Option<Position2>,
    ) -> Result<Option<RuntimeReport>, StepGameplayError> {
        self.ensure_solo()?;
        self.validate_audio_operation(now, audio_at, position)?;
        loop {
            let Some(prepared) = self.retained_input(merger, now, audio_at)? else {
                return Ok(None);
            };
            let state = self.retained.as_ref().expect("retained branch");
            let phase = state.phase_point(prepared.output(), prepared.query())?;
            let prefix = self
                .authority
                .as_ref()
                .expect("audio mode")
                .acquired_prefix()
                .expect("prepared acquired input");
            let position = position.or(merger
                .peek_ready_position(prefix)
                .map_err(AudioAuthorityError::from)?);
            let event = merger
                .pop_ready(prefix)
                .map_err(AudioAuthorityError::from)?
                .expect("prepared earliest input");
            if state.paused || state.ended {
                self.authority
                    .as_mut()
                    .expect("audio mode")
                    .commit_coordinate_input(prepared, now)?;
                continue;
            }
            self.started = true;
            let result = match position {
                Some(position) => self.runtime.solo_mut()?.process_input_at_output_phase(
                    event,
                    position,
                    prepared.mapper(),
                    audio_at,
                    phase,
                ),
                None => self.runtime.solo_mut()?.process_input_output_phase(
                    event,
                    prepared.mapper(),
                    audio_at,
                    phase,
                ),
            };
            if result.is_ok() {
                self.authority
                    .as_mut()
                    .expect("audio mode")
                    .commit_coordinate_input(prepared, now)?;
            }
            return self.observe(result).map(Some);
        }
    }
    fn advance_retained_frontier(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<RuntimeReport>, StepGameplayError> {
        self.ensure_solo()?;
        self.validate_audio_operation(now, audio_at, None)?;
        let state = self.retained.as_ref().expect("retained branch");
        if !state.started {
            return Ok(None);
        }
        let authority = self.authority.as_mut().expect("audio mode");
        if state.paused || state.ended {
            if let Some(frontier) = authority.prepare_coordinate_held_frontier(now, merger)? {
                authority.commit_coordinate_held_frontier(frontier, now, merger)?;
            }
            return Ok(None);
        }
        let Some(prepared) = authority.prepare_coordinate_frontier(now, merger)? else {
            return Ok(None);
        };
        if !state.permits_query(prepared.query())? {
            return Ok(None);
        }
        let Some(output) = prepared.advance() else {
            authority.commit_coordinate_frontier(prepared, now, merger)?;
            return Ok(None);
        };
        let phase = state.phase_point(prepared.output(), prepared.query())?;
        self.started = true;
        let result = self.runtime.solo_mut()?.advance_to_output_phase(
            output,
            &AudioDomains,
            audio_at,
            phase,
        );
        if result.is_ok() {
            self.authority
                .as_mut()
                .expect("audio mode")
                .commit_coordinate_frontier(prepared, now, merger)?;
            self.last_operation = Some(output);
        }
        self.observe(result).map(Some)
    }
    fn retained_cut(
        &self,
        merger: &InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<
        Option<(
            WorkletRetainedEnvelope,
            crate::audio_authority::PreparedCoordinateControlCutoff,
            OutputPhasePoint,
        )>,
        StepGameplayError,
    > {
        self.validate_audio_operation(now, audio_at, None)?;
        let state = self
            .retained
            .as_ref()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "retained practice is unavailable",
            ))?;
        let Some((event, false)) = state.event else {
            return Ok(None);
        };
        let Some(history) = &state.history else {
            return Ok(None);
        };
        if history.context_start != event.context_start {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained event belongs to another AudioContext arm",
            ));
        }
        // Worklet projections retain their own relative Mixer origin. Only the
        // actual coordinate authority joins this exact physical duration to the
        // caller's distinct raw-output and normalized logical clock origins.
        let query = retained_event_query(event, self.sample_rate)?;
        let span = history.span_at(query)?;
        let frame = state.event_frame(event);
        let playback = match span.state() {
            SourcePlaybackState::Frozen => Some(span.playback_start()),
            SourcePlaybackState::Running => span.playback_start().checked_add(
                frame
                    .checked_sub(span.physical_start())
                    .ok_or(StepGameplayError::SequenceOverflow)?,
            ),
        };
        let actual_playback = match event.event {
            WorkletRetainedEvent::Pause(pause) => pause.playback_frame,
            WorkletRetainedEvent::Practice(projected) => match projected.event {
                PracticeEvent::Boundary(receipt) => receipt.playback_frame,
                PracticeEvent::Rate(rate) => rate.playback_frame,
                PracticeEvent::Restart(restart) => restart.boundary.playback_frame,
            },
        };
        if playback != Some(actual_playback) {
            return Err(StepGameplayError::InvalidConfiguration(
                "retained event playback differs from actual source span",
            ));
        }
        let authority = self.authority.as_ref().expect("retained authority");
        let Some(host) = authority.presented_coordinate_boundary_host(&query, now)? else {
            return Ok(None);
        };
        let resume = matches!(event.event, WorkletRetainedEvent::Pause(pause) if !pause.paused)
            || matches!(event.event, WorkletRetainedEvent::Practice(projected) if matches!(projected.event, PracticeEvent::Boundary(receipt) if receipt.kind == PracticeBoundaryKind::Started));
        // The first integer HOST at a fractional cut can map strictly after it.
        // Keep that genuine input queued for the new phase, while exact-cut
        // equality retains the ordinary inclusive drain policy. Only prepare
        // an actual queued input; a closed prefix need not admit a new input.
        let after_cut_input = match (resume, authority.acquired_prefix()) {
            (false, Some(prefix)) => {
                match merger
                    .peek_ready(prefix)
                    .map_err(AudioAuthorityError::from)?
                {
                    Some(input) if input.meta().timestamp == host.timestamp => authority
                        .prepare_coordinate_input(
                            ClockPoint {
                                domain: input.meta().clock_domain,
                                timestamp: input.meta().timestamp,
                            },
                            now,
                        )?
                        .is_some_and(|prepared| {
                            prepared.query().compare_interpolated_time(query).is_gt()
                        }),
                    _ => false,
                }
            }
            _ => false,
        };
        let cutoff = if resume || after_cut_input {
            authority.prepare_coordinate_resume_control_cutoff(
                authority.epoch().id,
                query,
                host,
                now,
                merger,
            )?
        } else {
            authority.prepare_coordinate_control_cutoff(
                authority.epoch().id,
                query,
                host,
                now,
                merger,
            )?
        };
        let Some(cutoff) = cutoff else {
            return Ok(None);
        };
        let point = state.phase_point(cutoff.output(), query)?;
        authority.validate_coordinate_control_cutoff(&cutoff, now, merger)?;
        Ok(Some((event, cutoff, point)))
    }
    pub fn service_retained_event(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<u64>, StepGameplayError> {
        self.ensure_solo()?;
        if let Some((event, true)) = self.retained.as_ref().and_then(|state| state.event) {
            return Ok(Some(event.serial));
        }
        let Some((event, cutoff, point)) = self.retained_cut(merger, now, audio_at)? else {
            return Ok(None);
        };
        match event.event {
            WorkletRetainedEvent::Practice(projected)
                if !matches!(projected.event, PracticeEvent::Rate(_)) =>
            {
                let update = self
                    .retained
                    .as_ref()
                    .expect("retained branch")
                    .boundary_update(projected.event, self.end)?;
                if matches!(
                    update.receipt.kind,
                    PracticeBoundaryKind::Looped | PracticeBoundaryKind::Ended
                ) && self
                    .runtime
                    .transport_mut()
                    .position_at_output_phase(point)
                    .map_err(retained_error)?
                    != update.region.end
                {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "actual retained endpoint differs from exact active phase",
                    ));
                }
                if update.fresh {
                    if !self
                        .retained
                        .as_ref()
                        .expect("retained branch")
                        .retired
                        .is_empty()
                    {
                        return Ok(None);
                    }
                    self.install_retained_solo_attempt(&update, cutoff.output())?;
                } else {
                    if update.receipt.kind == PracticeBoundaryKind::Ended {
                        let result = self.runtime.solo_mut()?.advance_to_output_phase(
                            cutoff.output(),
                            &AudioDomains,
                            audio_at,
                            point,
                        );
                        self.observe(result)?;
                    }
                    self.retained
                        .as_mut()
                        .expect("retained branch")
                        .commit_boundary(&update)?;
                    if update.receipt.kind == PracticeBoundaryKind::Ended {
                        self.runtime.seal_gameplay_audio();
                    }
                }
                self.started = true;
            }
            _ => {
                let state = self.retained.as_mut().expect("retained branch");
                state.commit_control(
                    event,
                    point,
                    self.runtime.transport_mut(),
                    &mut [(PlayerId(1), &mut self.capture)],
                )?;
            }
        }
        self.authority
            .as_mut()
            .expect("retained authority")
            .commit_coordinate_control_cutoff(cutoff, now, merger)
            .expect("private preflight remains unchanged during runtime commit");
        self.retained.as_mut().expect("retained branch").event = Some((event, true));
        self.last_operation = Some(cutoff.output());
        Ok(Some(event.serial))
    }
    fn install_retained_solo_attempt(
        &mut self,
        update: &RetainedBoundaryUpdate,
        logical: ClockPoint,
    ) -> Result<(), StepGameplayError> {
        let state = self.retained.as_ref().expect("retained branch");
        let source = crate::section_start::source_at(&state.original, update.region.start)
            .map_err(retained_error)?;
        let fresh = prepare_fresh_step_member(
            state,
            0,
            &source,
            self.runtime.judge().profile().clone(),
            self.gauge.profile(),
            self.policy.as_ref(),
            self.input_mode,
            update,
        )?;
        let basis = OutputPhaseBasis::new(
            self.logical_domain,
            state.basis.owner(),
            update.receipt.generation,
            update.receipt.playback_frame,
            self.sample_rate,
        )
        .map_err(retained_error)?;
        let phase = PositiveRatePhase::new(
            update.region.start,
            Duration::ZERO,
            update.rate.rate,
            update.end,
        )
        .map_err(retained_error)?;
        let transport = Transport::new_with_runtime_output_phase(
            basis,
            logical,
            update.region.start,
            Duration::ZERO,
            update.rate.rate,
            update.end,
        )
        .map_err(retained_error)?;
        let headers = vec![fresh.header];
        let captured = if fresh.capture.is_some() {
            vec![PlayerId(1)]
        } else {
            Vec::new()
        };
        let business = state
            .business
            .candidate_for_attempt(headers.clone(), &captured, phase, update.rate)
            .map_err(retained_error)?;
        let setup = LiveSpeedSetup::new(
            update.region.start,
            Duration::ZERO,
            ReplaySpeed::new(update.rate.rate).map_err(retained_error)?,
            update.end,
        )
        .map_err(retained_error)?;
        let sound_identity = InputSoundIdentity::from_source(&source).map_err(retained_error)?;
        let completion = state
            .completion_template
            .prepare_practice(&source, &fresh.judge)
            .map_err(retained_error)?;
        let original_rate = state.setup.config.initial_rate;
        if update.restart {
            self.runtime
                .solo_mut()?
                .replace_practice_state_with_runtime_restart(
                    fresh.judge,
                    transport,
                    update.end,
                    CommandScope(update.receipt.generation),
                    original_rate,
                )
        } else {
            self.runtime
                .solo_mut()?
                .replace_practice_state_with_output_phase(
                    fresh.judge,
                    transport,
                    update.end,
                    CommandScope(update.receipt.generation),
                )
        }
        .map_err(retained_error)?;
        let state = self.retained.as_mut().expect("retained branch");
        if let Some(old) = self.capture.take() {
            state.retired.push((PlayerId(1), old));
        }
        self.capture = fresh.capture;
        self.gauge = fresh.gauge;
        self.score = ScoreSummary::default();
        self.mine_damage = MineDamageSummary::default();
        self.completed_result = None;
        self.live_terminal = false;
        self.start = update.region.start;
        self.end = update.end;
        self.preroll = Duration::ZERO;
        self.live_speed = Some(setup);
        self.input_sound_identity = sound_identity;
        self.completion = Some(completion);
        self.song = update.region.start;
        state.business = business;
        state.phase = phase;
        state.basis = basis;
        state.headers = headers;
        state
            .commit_boundary(update)
            .expect("fresh candidate already contains the actual revision");
        Ok(())
    }
    pub fn new(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::new_at(prepared, config, bindings, Timestamp::ZERO)
    }

    /// Own a fresh section already selected by `section_start::prepare_at` from
    /// original assets. Chart targets and PCM stay untouched; only BGM output
    /// scheduling subtracts this immutable original-song start.
    pub fn new_at(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::new_section(prepared, config, bindings, start, None)
    }

    /// Own an already selected section with an optional immutable original-song
    /// end. The shared runtime caps logical processing; actual output must use
    /// the returned playback endpoint on this configuration's output grid.
    pub fn new_section(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::new_section_with_input_mode(
            prepared,
            config,
            bindings,
            start,
            end,
            BmsInputMode::ButtonOnly,
        )
    }

    /// Cold selected speed with the default button-only input mode.
    pub fn new_section_with_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::new_section_with_input_mode_and_speed(
            prepared,
            config,
            bindings,
            start,
            end,
            BmsInputMode::ButtonOnly,
            speed,
        )
    }

    /// Own the actual section runtime with an explicit, replayable input mode.
    /// Bindings retain original physical events; the selected core rules judge them.
    pub fn new_section_with_input_mode(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            None,
            None,
            None,
        )
    }

    /// Fresh solo gameplay normalized on the logical audio timeline.
    pub fn new_audio_section(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            Some(authority),
            None,
            None,
        )
    }

    /// Uses an immutable selected policy for actual stages, gauge and capture.
    /// Config offset must agree; policy windows replace config early/late bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn new_section_with_policy(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        policy: crate::play_policy::ResolvedPlayPolicy,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            None,
            Some(policy),
            None,
        )
    }

    /// Policy-aware preparation on the actual logical audio authority timeline.
    #[allow(clippy::too_many_arguments)]
    pub fn new_audio_section_with_policy(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        policy: crate::play_policy::ResolvedPlayPolicy,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            Some(authority),
            Some(policy),
            None,
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_section_with_input_mode_and_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            None,
            None,
            Some(speed),
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_audio_section_with_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            Some(authority),
            None,
            Some(speed),
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_section_with_policy_and_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        policy: crate::play_policy::ResolvedPlayPolicy,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            None,
            Some(policy),
            Some(speed),
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_audio_section_with_policy_and_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        bindings: BindingMap,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        policy: crate::play_policy::ResolvedPlayPolicy,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build(
            prepared,
            config,
            InputSetup::Solo(bindings),
            start,
            end,
            input_mode,
            Some(authority),
            Some(policy),
            Some(speed),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        input: InputSetup,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: Option<AudioAuthority>,
        policy: Option<crate::play_policy::ResolvedPlayPolicy>,
        speed: Option<ReplaySpeed>,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        Self::build_mode(
            prepared, config, input, start, end, input_mode, authority, policy, speed, false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_mode(
        mut prepared: PreparedBms,
        config: StepGameplayConfig,
        input: InputSetup,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: Option<AudioAuthority>,
        policy: Option<crate::play_policy::ResolvedPlayPolicy>,
        speed: Option<ReplaySpeed>,
        retained: bool,
    ) -> Result<(Self, SampleBank), StepGameplayError> {
        let logical_origin = if let Some(authority) = &authority {
            let epoch = authority.epoch();
            if epoch.host_domain != config.host_origin.domain
                || epoch.stream_origin != config.output_origin
                || authority.committed_input_host().is_some()
                || authority.committed_operation().is_some()
                || authority.committed_presentation().is_some()
                || authority.closed_host_prefix().is_some()
                || epoch
                    .logical_origin
                    .timestamp
                    .checked_add(config.preroll)
                    .is_none()
            {
                return Err(StepGameplayError::InvalidConfiguration(
                    "audio setup requires matching pristine origins",
                ));
            }
            epoch.logical_origin
        } else {
            config.host_origin
        };
        if config.host_origin.domain == config.output_origin.domain {
            return Err(StepGameplayError::InvalidConfiguration(
                "host and output domains must be distinct",
            ));
        }
        if config.preroll.as_nanos() < 0 || config.bgm_lookahead.as_nanos() <= 0 {
            return Err(StepGameplayError::InvalidConfiguration(
                "nonnegative preroll and positive BGM lookahead required",
            ));
        }
        if !(1..=AudioLimits::MAX_COMMANDS).contains(&config.command_capacity)
            || config.bgm_pending == 0
            || config.bgm_pending >= config.command_capacity
            || config.telemetry_capacity > 65_536
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "invalid command, BGM or telemetry capacity",
            ));
        }
        if end.is_some_and(|end| end <= start) {
            return Err(StepGameplayError::InvalidConfiguration(
                "finite section end must be strictly later than start",
            ));
        }
        if start.as_nanos() < 0 {
            return Err(StepGameplayError::InvalidConfiguration(
                "section start must be nonnegative",
            ));
        }
        // The zero-start entry point preserves its existing preparation policy.
        // A positive section must have removed earlier heads (including entire
        // crossing holds), and selected any overlapping BGM PCM before arrival.
        if start != Timestamp::ZERO
            && (prepared
                .compiled
                .chart
                .objects()
                .iter()
                .any(|object| object.time.start < start)
                || (!retained
                    && prepared
                        .bgm_commands
                        .iter()
                        .any(|command| command.at() < start)))
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "section contains an earlier object or unselected BGM target",
            ));
        }
        let live_speed = speed
            .map(|speed| LiveSpeedSetup::new(start, config.preroll, speed, end))
            .transpose()
            .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
        let song = if let Some(setup) = live_speed {
            setup.song_origin()
        } else {
            start
                .checked_sub(config.preroll)
                .ok_or(StepGameplayError::InvalidConfiguration(
                    "section start minus preroll cannot be represented",
                ))?
        };
        if config
            .host_origin
            .timestamp
            .checked_add(config.preroll)
            .is_none()
            || config
                .output_origin
                .timestamp
                .checked_add(config.preroll)
                .is_none()
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "section start exceeds the host or output clock range",
            ));
        }
        let profile = if let Some(policy) = &policy {
            if policy.judge().input_offset().as_nanos() != config.offset_ns {
                return Err(StepGameplayError::InvalidConfiguration(
                    "selected policy offset differs from config",
                ));
            }
            if let Some(timing) = policy.timing() {
                let declared = prepared
                    .source
                    .judge_rank_metadata()
                    .map_err(|error| StepGameplayError::Setup(error.to_string()))?
                    .resolve(timing.selection().precedence);
                if declared != Some(timing.profiles().difficulty()) {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "selected timing differs from original source declaration",
                    ));
                }
            }
            policy.judge().clone()
        } else {
            NativeJudgeConfig {
                early: config.early_ns,
                late: config.late_ns,
                offset: config.offset_ns,
                preroll: config.preroll.as_nanos(),
                output: config.output_origin.domain,
                end: None,
            }
            .profile()
            .map_err(|error| StepGameplayError::Setup(error.to_string()))?
        };
        let sample_rate = prepared.bank.format().sample_rate();
        let playback_end_frame = if retained {
            None
        } else if let Some(setup) = live_speed {
            setup
                .playback_end_frame(sample_rate)
                .map_err(|error| StepGameplayError::Setup(error.to_string()))?
        } else {
            end.map(|end| section_end_frame(start, end, config.preroll, sample_rate))
                .transpose()
                .map_err(StepGameplayError::Setup)?
        };
        if playback_end_frame.is_some() {
            validate_section_output_evidence(
                config.output_origin,
                sample_rate,
                None,
                None,
                None,
                None,
                playback_end_frame,
            )
            .map_err(|error| StepGameplayError::Completion {
                error,
                rendered: None,
                presented: None,
            })?;
        }
        let completion = if end.is_none() && !retained {
            Some(
                SongCompletion::prepare(
                    &prepared,
                    policy
                        .as_ref()
                        .map_or(config.late_ns, |policy| policy.completion_late().as_nanos()),
                    profile.input_offset().as_nanos(),
                    config.preroll.as_nanos(),
                    config.output_origin.domain,
                )
                .map_err(|error| StepGameplayError::Setup(error.to_string()))?,
            )
        } else {
            None
        };
        let input_sound_identity =
            InputSoundIdentity::from_source(&prepared.source).map_err(StepGameplayError::Setup)?;
        // Local voice reservation sees original BGM IDs before section mapping
        // or filtering. The legacy solo branch retains its original sound IDs.
        let runtime_setup = match input {
            InputSetup::Solo(bindings) => {
                let input_sounds = if prepared.source.invisible.is_empty() {
                    None
                } else {
                    let plan = InputSoundPlan::prepare(
                        &prepared.source,
                        &prepared.sounds,
                        &prepared.bgm_commands,
                        beatkernel_bms::ParseOptions::default().max_objects,
                    )
                    .map_err(StepGameplayError::Setup)?;
                    for &sample in plan.samples() {
                        if prepared.bank.get(sample).is_none() {
                            return Err(StepGameplayError::InvalidConfiguration(
                                "input sound PCM sample is missing",
                            ));
                        }
                    }
                    Some(plan.timeline())
                };
                let hazard_sounds = if prepared.source.mines.is_empty() {
                    None
                } else {
                    let mine_sounds = MineSoundPlan::prepare(
                        &prepared.source,
                        &prepared.sounds,
                        &prepared.bgm_commands,
                        input_sounds.as_ref(),
                        beatkernel_bms::ParseOptions::default().max_objects,
                    )
                    .map_err(StepGameplayError::Setup)?;
                    for &sample in mine_sounds.samples() {
                        if prepared.bank.get(sample).is_none() {
                            return Err(StepGameplayError::InvalidConfiguration(
                                "mine sound PCM sample is missing",
                            ));
                        }
                    }
                    mine_sounds.timeline()
                };
                let judge = prepare_judge_with_timing(
                    &prepared.source,
                    prepared.compiled.chart,
                    profile,
                    input_mode,
                    beatkernel_bms::ParseOptions::default().max_objects,
                    policy
                        .as_ref()
                        .and_then(|policy| policy.timing())
                        .map(|timing| timing.profiles()),
                )
                .map_err(StepGameplayError::Setup)?;
                if let Some(policy) = &policy {
                    policy
                        .validate_timing(&judge, input_mode)
                        .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
                }
                RuntimeSetup::Solo {
                    bindings,
                    judge,
                    input_sounds,
                    hazard_sounds,
                }
            }
            InputSetup::Local(plan, bindings) => {
                let primary = plan.members()[0].0;
                let members = prepare_local_members_with_timing(
                    &prepared,
                    &plan,
                    bindings,
                    profile,
                    input_mode,
                    policy
                        .as_ref()
                        .and_then(|policy| policy.timing())
                        .map(|timing| timing.profiles()),
                )
                .map_err(StepGameplayError::Setup)?;
                if let Some(policy) = &policy {
                    for member in &members.configs {
                        policy
                            .validate_timing(&member.judge, input_mode)
                            .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
                    }
                }
                let input_sounds =
                    prepare_local_input_sounds(&prepared, &members.configs, &members.reserved)
                        .map_err(StepGameplayError::Setup)?;
                let hazard_sounds = prepare_local_mine_sounds(
                    &prepared,
                    &members.configs,
                    &members.reserved,
                    &input_sounds,
                )
                .map_err(StepGameplayError::Setup)?;
                RuntimeSetup::Local {
                    members,
                    primary,
                    input_sounds,
                    hazard_sounds,
                }
            }
        };
        // Original cues have already reserved the shared voice namespace above.
        // The retained Worklet is their sole scheduler.
        if retained {
            prepared.bgm_commands.clear();
        }
        let bgm_count = prepared.bgm_commands.len();
        if let Some(end) = end {
            for command in &prepared.bgm_commands {
                if !matches!(command, AudioCommand::Play { gain, .. } if gain.is_finite()) {
                    return Err(StepGameplayError::Bgm {
                        error: BgmFeedError::InvalidCommand(*command),
                        report: BgmFeedReport {
                            remaining: bgm_count,
                            ..BgmFeedReport::default()
                        },
                    });
                }
            }
            // Future full-chart cues cannot reach this section. Discard them
            // before output conversion so an irrelevant distant cue cannot
            // overflow the finite run's otherwise representable mapping.
            prepared
                .bgm_commands
                .retain(|command| !matches!(command, AudioCommand::Play { at, .. } if *at >= end));
        }
        if start != Timestamp::ZERO && live_speed.is_none() {
            for command in &mut prepared.bgm_commands {
                if let AudioCommand::Play { at, .. } = command {
                    let relative = i128::from(at.as_nanos()) - i128::from(start.as_nanos());
                    *at = Timestamp::from_nanos(i64::try_from(relative).map_err(|_| {
                        StepGameplayError::Bgm {
                            error: BgmFeedError::Overflow,
                            report: BgmFeedReport {
                                remaining: bgm_count,
                                ..BgmFeedReport::default()
                            },
                        }
                    })?);
                }
            }
        }
        let mut bgm_config = BgmConfig {
            output_origin: config.output_origin,
            sample_rate,
            preroll: config.preroll,
            lookahead: config.bgm_lookahead,
            max_pending: config.bgm_pending,
        };
        let bgm = if let Some(setup) = live_speed {
            let mut mapped = Vec::new();
            let include_rate = setup.speed().rate() != Rate::NORMAL
                && before_endpoint(
                    config.output_origin.timestamp,
                    config.output_origin,
                    sample_rate,
                    playback_end_frame,
                )
                .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
            mapped
                .try_reserve_exact(prepared.bgm_commands.len() + usize::from(include_rate))
                .map_err(|_| StepGameplayError::AllocationFailed)?;
            if include_rate {
                mapped.push(AudioCommand::SetRate {
                    at: config.output_origin.timestamp,
                    rate: setup.speed().rate(),
                });
            }
            for command in prepared.bgm_commands {
                let AudioCommand::Play {
                    voice,
                    sample,
                    at,
                    gain,
                } = command
                else {
                    return Err(StepGameplayError::Bgm {
                        error: BgmFeedError::InvalidCommand(command),
                        report: BgmFeedReport {
                            remaining: bgm_count,
                            ..BgmFeedReport::default()
                        },
                    });
                };
                if !gain.is_finite() {
                    return Err(StepGameplayError::Bgm {
                        error: BgmFeedError::InvalidCommand(command),
                        report: BgmFeedReport {
                            remaining: bgm_count,
                            ..BgmFeedReport::default()
                        },
                    });
                }
                let at = setup
                    .output_at(config.output_origin, at)
                    .map_err(|error| StepGameplayError::Setup(error.to_string()))?
                    .timestamp;
                if before_endpoint(at, config.output_origin, sample_rate, playback_end_frame)
                    .map_err(|error| StepGameplayError::Setup(error.to_string()))?
                {
                    mapped.push(AudioCommand::Play {
                        voice,
                        sample,
                        at,
                        gain,
                    });
                }
            }
            bgm_config.preroll = Duration::ZERO;
            BgmFeeder::from_output_commands_with_rate(mapped, bgm_config, setup.speed().rate())
        } else if playback_end_frame.is_some() {
            let mut kept = 0;
            for index in 0..prepared.bgm_commands.len() {
                let mut command = prepared.bgm_commands[index];
                if let AudioCommand::Play { at, .. } = &mut command {
                    let mapped = i128::from(config.output_origin.timestamp.as_nanos())
                        + i128::from(at.as_nanos())
                        + i128::from(config.preroll.as_nanos());
                    *at = Timestamp::from_nanos(i64::try_from(mapped).map_err(|_| {
                        StepGameplayError::Bgm {
                            error: BgmFeedError::Overflow,
                            report: BgmFeedReport {
                                remaining: bgm_count,
                                ..BgmFeedReport::default()
                            },
                        }
                    })?);
                }
                if command.at() < config.output_origin.timestamp {
                    return Err(StepGameplayError::Bgm {
                        error: BgmFeedError::InvalidCommand(command),
                        report: BgmFeedReport {
                            remaining: bgm_count,
                            ..BgmFeedReport::default()
                        },
                    });
                }
                let keep = before_endpoint(
                    command.at(),
                    config.output_origin,
                    sample_rate,
                    playback_end_frame,
                )
                .map_err(|_| StepGameplayError::Bgm {
                    error: BgmFeedError::Overflow,
                    report: BgmFeedReport {
                        remaining: bgm_count,
                        ..BgmFeedReport::default()
                    },
                })?;
                if keep {
                    prepared.bgm_commands[kept] = command;
                    kept += 1;
                }
            }
            prepared.bgm_commands.truncate(kept);
            bgm_config.preroll = Duration::ZERO;
            BgmFeeder::from_output_commands(prepared.bgm_commands, bgm_config)
        } else {
            BgmFeeder::new(prepared.bgm_commands, bgm_config)
        }
        .map_err(|error| StepGameplayError::Bgm {
            error,
            report: BgmFeedReport {
                remaining: bgm_count,
                ..BgmFeedReport::default()
            },
        })?;
        let (producer, consumer) = command_queue(config.command_capacity)
            .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
        let transport = if let Some(setup) = live_speed {
            setup
                .transport(logical_origin.timestamp)
                .map_err(|error| StepGameplayError::Setup(error.to_string()))?
        } else {
            Transport::new(logical_origin.timestamp, song, Rate::NORMAL)
        };
        let mut runtime = match runtime_setup {
            RuntimeSetup::Solo {
                bindings,
                judge,
                input_sounds,
                hazard_sounds,
            } => {
                let mut solo = SoloRuntime::new(
                    logical_origin.domain,
                    config.output_origin.domain,
                    transport,
                    bindings,
                    judge,
                    producer,
                    prepared.sounds,
                    config.telemetry_capacity,
                )
                .map_err(StepGameplayError::Setup)?;
                if let Some(timeline) = input_sounds {
                    solo.configure_input_sounds(timeline)
                        .map_err(StepGameplayError::Setup)?;
                }
                if let Some(timeline) = hazard_sounds {
                    solo.configure_hazard_sounds(timeline)
                        .map_err(StepGameplayError::Setup)?;
                }
                RuntimeOwner::Solo(solo)
            }
            RuntimeSetup::Local {
                members,
                primary,
                input_sounds,
                hazard_sounds,
            } => {
                let mut group = RuntimeGroup::new(
                    logical_origin.domain,
                    config.output_origin.domain,
                    transport,
                    producer,
                    members.configs,
                    config.telemetry_capacity,
                    &members.reserved,
                )
                .map_err(StepGameplayError::Setup)?;
                if !input_sounds.is_empty() {
                    group
                        .configure_input_sounds(input_sounds)
                        .map_err(StepGameplayError::Setup)?;
                }
                if !hazard_sounds.is_empty() {
                    group
                        .configure_hazard_sounds(hazard_sounds)
                        .map_err(StepGameplayError::Setup)?;
                }
                RuntimeOwner::Local { group, primary }
            }
        };
        // Profiling is not a clock source. In particular, no std::time::Instant
        // is sampled by core processing on browser or other nonblocking hosts.
        runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
        if let Some(end) = end {
            runtime
                .set_song_end(end)
                .map_err(StepGameplayError::Setup)?;
        }
        let mut owner = Self {
            runtime,
            consumer,
            bgm,
            completion,
            output_origin: config.output_origin,
            sample_rate,
            last_render: None,
            last_presented: None,
            output_clock: None,
            authority,
            correction_watermark: None,
            capture: None,
            capture_configured: false,
            score: ScoreSummary::default(),
            mine_damage: MineDamageSummary::default(),
            gauge: BmsGauge::new(match &policy {
                Some(policy) => policy
                    .gauge()
                    .try_copy()
                    .map_err(|error| StepGameplayError::Setup(error.to_string()))?,
                None => crate::gauge::GaugeProfile::default(),
            }),
            policy,
            completed_result: None,
            song,
            host_domain: config.host_origin.domain,
            logical_domain: logical_origin.domain,
            start,
            end,
            input_mode,
            input_sound_identity,
            playback_end_frame,
            preroll: config.preroll,
            live_speed,
            last_operation: None,
            pre_origin_inputs: 0,
            live_terminal: false,
            activated: false,
            started: false,
            pending: None,
            sequence: 0,
            acknowledged_commands: 0,
            acknowledged_stops: OwnedStopEvidence::default(),
            failed: false,
            retained: None,
        };
        owner.feed_audio(0, config.bgm_pending)?;
        Ok((owner, prepared.bank))
    }

    fn ensure_usable(&self) -> Result<(), StepGameplayError> {
        if self.failed {
            Err(StepGameplayError::Failed)
        } else {
            Ok(())
        }
    }

    fn ensure_processing(&self) -> Result<(), StepGameplayError> {
        if self.live_terminal {
            return Err(StepGameplayError::InvalidConfiguration(
                "finite live playback is completed",
            ));
        }
        Ok(())
    }

    fn ensure_solo(&self) -> Result<(), StepGameplayError> {
        if matches!(self.runtime, RuntimeOwner::Solo(_)) {
            Ok(())
        } else {
            Err(StepGameplayError::InvalidConfiguration(
                "scalar gameplay cannot operate a local cohort",
            ))
        }
    }

    fn reset_drain(&mut self) {
        if let Some(completion) = &mut self.completion {
            completion.reset_drain();
        }
    }

    /// Normalized pristine network setup, without enabling capture.
    /// Retains the original-song start; the finite end belongs to the identity envelope.
    pub fn competition_header(
        &self,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<ReplayHeader, StepGameplayError> {
        self.setup_header(limits, chart_seed, None)
    }

    /// Capture-exact pristine metadata for saved comparisons, without enabling capture.
    pub(crate) fn recording_header(
        &self,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<ReplayHeader, StepGameplayError> {
        self.setup_header(limits, chart_seed, self.end)
    }

    fn setup_header(
        &self,
        limits: ReplayCodecLimits,
        chart_seed: u64,
        end: Option<Timestamp>,
    ) -> Result<ReplayHeader, StepGameplayError> {
        self.ensure_solo()?;
        self.ensure_usable()?;
        if self.started {
            return Err(StepGameplayError::InvalidConfiguration(
                "competition identity requires an unprocessed runtime",
            ));
        }
        if let Some(policy) = &self.policy {
            crate::replay_capture::setup_play_policy_header(
                self.runtime.judge(),
                self.logical_domain,
                limits,
                self.start,
                chart_seed,
                end,
                self.input_mode,
                self.input_sound_identity,
                policy,
            )
        } else {
            setup_gauge_header(
                self.runtime.judge(),
                self.logical_domain,
                limits,
                self.start,
                chart_seed,
                end,
                self.input_mode,
                self.input_sound_identity,
                self.gauge.profile(),
            )
        }
        .and_then(|header| {
            crate::replay_speed_policy::wrap_header(header, self.playback_speed(), limits)
                .map_err(CaptureError::SourceSpeed)
        })
        .map_err(|error| StepGameplayError::Capture {
            error,
            report: None,
        })
    }

    /// Exact native-compatible setup identity without enabling capture or
    /// changing the pristine judge. Host/output origins, preroll and capture
    /// choice do not change identity.
    pub fn competition_identity(
        &self,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<Vec<u8>, StepGameplayError> {
        let header = self.competition_header(limits, chart_seed)?;
        crate::multiplayer::competition_identity_for_section(
            &header,
            env!("CARGO_PKG_VERSION"),
            limits,
            self.end,
        )
        .map_err(|error| StepGameplayError::Setup(error.to_string()))
    }

    /// Installs a resolved gauge before activation, processing or capture identity.
    pub fn configure_gauge(
        &mut self,
        profile: crate::gauge::GaugeProfile,
    ) -> Result<(), StepGameplayError> {
        self.ensure_solo()?;
        self.ensure_usable()?;
        if !self.input_setup_available() || self.capture_configured {
            return Err(StepGameplayError::InvalidConfiguration(
                "gauge configuration requires pristine setup before capture",
            ));
        }
        if self
            .policy
            .as_ref()
            .is_some_and(|policy| policy.gauge() != &profile)
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "gauge differs from immutable selected policy",
            ));
        }
        let replacement = if let Some(state) = &self.retained {
            let mut headers = state.headers.clone();
            if self.policy.is_none() {
                headers[0].header = setup_gauge_header(
                    self.runtime.judge(),
                    self.logical_domain,
                    state.options.identity_limits,
                    self.start,
                    state.options.chart_seed,
                    self.end,
                    self.input_mode,
                    self.input_sound_identity,
                    &profile,
                )
                .and_then(|header| {
                    crate::replay_speed_policy::wrap_header(
                        header,
                        state.business.current_speed(),
                        state.options.identity_limits,
                    )
                    .map_err(CaptureError::SourceSpeed)
                })
                .map_err(|error| StepGameplayError::Capture {
                    error,
                    report: None,
                })?;
            }
            let business =
                RuntimePractice::new(headers.clone(), &[], state.phase).map_err(retained_error)?;
            Some((headers, business))
        } else {
            None
        };
        self.gauge = BmsGauge::new(profile);
        if let Some((headers, business)) = replacement {
            let state = self.retained.as_mut().expect("retained setup checked");
            state.headers = headers;
            state.business = business;
        }
        Ok(())
    }
    /// Opt in while the original judge is pristine, using the resolved chart
    /// branch seed. Setup refusal is atomic and leaves this owner usable.
    /// Preroll reports retain their actual song times before the section start;
    /// the existing replay header retains that original-song start unchanged.
    pub fn configure_capture(
        &mut self,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<(), StepGameplayError> {
        self.ensure_solo()?;
        self.ensure_usable()?;
        if self.started || self.capture_configured {
            return Err(StepGameplayError::InvalidConfiguration(
                "capture configuration requires an unprocessed, unconfigured runtime",
            ));
        }
        let capture = if let Some(policy) = &self.policy {
            LiveReplayCapture::new_with_policy_and_speed(
                self.runtime.judge(),
                self.logical_domain,
                limits,
                self.start,
                chart_seed,
                self.end,
                self.input_mode,
                self.input_sound_identity,
                policy,
                self.playback_speed(),
            )
        } else {
            LiveReplayCapture::new_with_gauge_and_speed(
                self.runtime.judge(),
                self.logical_domain,
                limits,
                self.start,
                chart_seed,
                self.end,
                self.input_mode,
                self.input_sound_identity,
                self.gauge.profile(),
                self.playback_speed(),
            )
        }
        .map_err(|error| StepGameplayError::Capture {
            error,
            report: None,
        })?;
        let business = if let Some(state) = &self.retained {
            if state.pending.is_some()
                || state.business.effective_header(PlayerId(1)) != Some(capture.header())
            {
                return Err(StepGameplayError::InvalidConfiguration(
                    "capture differs from retained effective identity",
                ));
            }
            Some(
                RuntimePractice::new(state.headers.clone(), &[PlayerId(1)], state.phase)
                    .map_err(retained_error)?,
            )
        } else {
            None
        };
        self.capture = Some(capture);
        self.capture_configured = true;
        if let Some(business) = business {
            let state = self.retained.as_mut().expect("checked retained mode");
            state.business = business;
            state.capture_limits[0] = Some(limits);
        }
        Ok(())
    }

    /// Cold, read-only archive export from actual latched completion before capture consumption.
    pub fn completed_archive(
        &self,
    ) -> Result<Option<Vec<u8>>, crate::result_archive::ArchiveError> {
        self.completed_archive_impl(None)
    }
    pub fn completed_archive_with_comparisons(
        &self,
        rows: &[(
            PlayerId,
            Option<&crate::competition_presentation::CompetitionSnapshot>,
        )],
    ) -> Result<Option<Vec<u8>>, crate::result_archive::ArchiveError> {
        self.completed_archive_impl(Some(rows))
    }
    fn completed_archive_impl(
        &self,
        comparisons: Option<
            &[(
                PlayerId,
                Option<&crate::competition_presentation::CompetitionSnapshot>,
            )],
        >,
    ) -> Result<Option<Vec<u8>>, crate::result_archive::ArchiveError> {
        if !self.capture_configured {
            return Ok(None);
        }
        let Some(result) = self.completed_result else {
            return Ok(None);
        };
        let capture = self
            .capture
            .as_ref()
            .ok_or(crate::result_archive::ArchiveError::Invalid(
                "completed capture already consumed",
            ))?;
        let identity = completed_archive_identity(PlayerId(1), capture, self.gauge.profile())?;
        let mut archive = crate::result_archive::ResultArchive::from_completed_with_scores(
            &[(PlayerId(1), result)],
            &[identity],
            &[(PlayerId(1), &self.score)],
        )?;
        if let Some(rows) = comparisons {
            archive.attach_comparisons(rows)?;
        }
        Ok(Some(crate::result_archive::encode_archive(&archive)?))
    }

    /// Take the canonical accepted prefix once, only after stop/failure fenced
    /// further gameplay. Disabled or already consumed capture returns None.
    /// Encoding failure also consumes this export attempt; no operations retry.
    pub fn take_replay(&mut self) -> Result<Option<Vec<u8>>, StepGameplayError> {
        self.ensure_solo()?;
        if !self.failed {
            return Err(StepGameplayError::InvalidConfiguration(
                "replay export requires a stopped or fenced runtime",
            ));
        }
        self.capture
            .take()
            .map(LiveReplayCapture::into_bytes)
            .transpose()
            .map_err(|error| StepGameplayError::Capture {
                error,
                report: None,
            })
    }

    /// Activate the pristine logical audio anchor without replacing it with HOST time.
    pub fn activate_audio(&mut self) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        if self.authority.is_none() || self.activated || self.started {
            return Err(StepGameplayError::InvalidConfiguration(
                "audio activation requires pristine audio mode",
            ));
        }
        self.activated = true;
        Ok(())
    }

    pub fn audio_authority(&self) -> Option<&AudioAuthority> {
        self.authority.as_ref()
    }

    pub fn pre_origin_inputs(&self) -> u64 {
        self.pre_origin_inputs
    }

    /// Drain original pre-origin inputs, returning the next playable frozen descriptor.
    pub fn discard_pre_origin_audio_inputs(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<PreparedInput>, StepGameplayError> {
        if self.retained.is_some() {
            return Ok(None);
        }
        loop {
            let Some(prepared) = self.prepare_audio_input(merger, now, audio_at, None)? else {
                return Ok(None);
            };
            if !prepared.is_before_origin() {
                return Ok(Some(prepared));
            }
            let count = self
                .pre_origin_inputs
                .checked_add(1)
                .ok_or(AudioAuthorityError::Overflow)?;
            let authority = self.authority.as_mut().expect("audio mode checked");
            let prefix = authority
                .acquired_prefix()
                .expect("prepared input has acquired prefix");
            authority.commit_discarded_input(prepared)?;
            merger
                .pop_ready(prefix)
                .map_err(AudioAuthorityError::from)?
                .expect("private preparation inspected this earliest input");
            self.pre_origin_inputs = count;
        }
    }

    pub fn observe_audio_output(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, StepGameplayError> {
        self.ensure_audio()?;
        self.authority
            .as_mut()
            .expect("audio mode checked")
            .observe(epoch, pair)
            .map_err(Into::into)
    }

    pub fn record_audio_prefix(&mut self, prefix: ClockPoint) -> Result<(), StepGameplayError> {
        self.ensure_audio()?;
        self.authority
            .as_mut()
            .expect("audio mode checked")
            .record_acquired_prefix(prefix)
            .map_err(Into::into)
    }

    pub fn process_next_audio_input(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
        position: Option<Position2>,
    ) -> Result<Option<RuntimeReport>, StepGameplayError> {
        if self.retained.is_some() {
            return self.process_next_retained_input(merger, now, audio_at, position);
        }
        self.ensure_solo()?;
        self.validate_audio_operation(now, audio_at, position)?;
        let before = self.pre_origin_inputs;
        let Some(prepared) = self.discard_pre_origin_audio_inputs(merger, now, audio_at)? else {
            return Ok(None);
        };
        let position = if before != self.pre_origin_inputs {
            match self
                .authority
                .as_ref()
                .and_then(AudioAuthority::acquired_prefix)
            {
                Some(prefix) => merger
                    .peek_ready_position(prefix)
                    .map_err(AudioAuthorityError::from)?,
                None => None,
            }
        } else {
            position
        };
        self.validate_audio_operation(now, audio_at, position)?;
        let prefix = self
            .authority
            .as_ref()
            .expect("audio mode checked")
            .acquired_prefix()
            .expect("prepared input has acquired prefix");
        let event = merger
            .pop_ready(prefix)
            .map_err(AudioAuthorityError::from)?
            .expect("private preparation inspected this earliest input");
        self.started = true;
        let runtime = self.runtime.solo_mut()?;
        let result = match position {
            Some(position) => {
                runtime.process_input_at(event, position, prepared.mapper(), audio_at)
            }
            None => runtime.process_input(event, prepared.mapper(), audio_at),
        };
        if result.is_ok() {
            self.commit_audio_input(prepared);
        }
        self.observe(result).map(Some)
    }

    pub fn advance_audio_frontier(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<RuntimeReport>, StepGameplayError> {
        if self.retained.is_some() {
            return self.advance_retained_frontier(merger, now, audio_at);
        }
        self.ensure_solo()?;
        self.validate_audio_operation(now, audio_at, None)?;
        let Some(prepared) = self
            .authority
            .as_ref()
            .expect("audio mode checked")
            .prepare_frontier(now, merger)?
        else {
            return Ok(None);
        };
        let Some(output) = prepared.advance() else {
            self.commit_audio_frontier(prepared, merger);
            return Ok(None);
        };
        self.started = true;
        let result = self
            .runtime
            .solo_mut()?
            .advance_to(output, &AudioDomains, audio_at);
        if result.is_ok() {
            self.last_operation = Some(output);
            self.commit_audio_frontier(prepared, merger);
        }
        self.observe(result).map(Some)
    }

    fn ensure_legacy_clock(&self) -> Result<(), StepGameplayError> {
        if self.authority.is_some() {
            Err(StepGameplayError::InvalidConfiguration(
                "audio mode requires audio-authoritative entrypoints",
            ))
        } else {
            Ok(())
        }
    }
    fn ensure_audio(&self) -> Result<&AudioAuthority, StepGameplayError> {
        self.ensure_usable()?;
        if !self.activated {
            return Err(StepGameplayError::InvalidConfiguration(
                "audio mode requires activation",
            ));
        }
        self.authority
            .as_ref()
            .ok_or(StepGameplayError::InvalidConfiguration(
                "audio authority is unavailable in legacy mode",
            ))
    }
    fn validate_audio_operation(
        &self,
        now: ClockPoint,
        audio_at: ClockPoint,
        position: Option<Position2>,
    ) -> Result<(), StepGameplayError> {
        self.ensure_processing()?;
        let authority = self.ensure_audio()?;
        if now.domain != authority.epoch().host_domain
            || audio_at.domain != self.output_origin.domain
        {
            return Err(AudioAuthorityError::DomainMismatch.into());
        }
        if position.is_some_and(|position| !position.x.is_finite() || !position.y.is_finite()) {
            return Err(StepGameplayError::InvalidConfiguration(
                "audio input position must be finite",
            ));
        }
        Ok(())
    }
    fn prepare_audio_input(
        &self,
        merger: &InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
        position: Option<Position2>,
    ) -> Result<Option<PreparedInput>, StepGameplayError> {
        self.validate_audio_operation(now, audio_at, position)?;
        let authority = self.authority.as_ref().expect("audio mode checked");
        let Some(prefix) = authority.acquired_prefix() else {
            return Ok(None);
        };
        let Some(event) = merger
            .peek_ready(prefix)
            .map_err(AudioAuthorityError::from)?
        else {
            return Ok(None);
        };
        authority
            .prepare_input(
                ClockPoint {
                    domain: event.meta().clock_domain,
                    timestamp: event.meta().timestamp,
                },
                now,
            )
            .map_err(Into::into)
    }
    fn commit_audio_input(&mut self, prepared: PreparedInput) {
        // The descriptor was prepared privately and Runtime cannot mutate this
        // authority. Every fallible commit check was preflighted before dispatch.
        self.authority
            .as_mut()
            .expect("audio mode checked")
            .commit_input(prepared)
            .expect("current private audio input preparation is valid");
    }
    fn commit_audio_frontier(&mut self, prepared: PreparedFrontier, merger: &mut InputMerger) {
        self.authority
            .as_mut()
            .expect("audio mode checked")
            .commit_frontier(prepared, merger)
            .expect("current private audio frontier preparation is valid");
    }

    /// Opt in before any processing. Configuration reserves the existing
    /// discipline's bounded storage; its accuracy remains Unknown. Atomic setup
    /// refusal leaves this owner usable and does not alter its nominal transport.
    pub fn configure_output_clock(
        &mut self,
        config: DisciplineConfig,
    ) -> Result<(), StepGameplayError> {
        self.ensure_legacy_clock()?;
        self.ensure_usable()?;
        if self.started || self.output_clock.is_some() {
            return Err(StepGameplayError::InvalidConfiguration(
                "output clock configuration requires an unprocessed, unconfigured runtime",
            ));
        }
        if self.live_speed.is_some() {
            return Err(StepGameplayError::InvalidConfiguration(
                "fixed-speed playback requires audio authority instead of normal-rate discipline",
            ));
        }
        let discipline =
            PresentationDiscipline::new(config, self.output_origin, self.host_domain, self.song)
                .map_err(StepGameplayError::Clock)?;
        self.output_clock = Some(discipline);
        Ok(())
    }

    /// Admit an actual absolute output/host relation after activation, without
    /// changing transport or input timestamps. A measured host point may be
    /// earlier than the estimated activation instant; native warmup/history
    /// checks remain authoritative. Unchanged output cannot refresh freshness.
    pub fn observe_output_clock(
        &mut self,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, StepGameplayError> {
        self.ensure_legacy_clock()?;
        self.ensure_usable()?;
        if !self.activated || self.output_clock.is_none() {
            return Err(self.clock_failure(DisciplineError::InvalidConfig));
        }
        if pair.source.domain != self.output_origin.domain || pair.target.domain != self.host_domain
        {
            return Err(self.clock_failure(DisciplineError::DomainMismatch));
        }
        if pair.source.timestamp < self.output_origin.timestamp {
            return Err(self.clock_failure(DisciplineError::NonIncreasing));
        }
        let result = self
            .output_clock
            .as_mut()
            .expect("configured discipline was checked")
            .observe_clock_pair(pair);
        result.map_err(|error| self.clock_failure(error))
    }

    /// Apply continuous correction only at the latest successfully accepted
    /// original host watermark, after its complete input prefix. Any later input
    /// invalidates that permission until another successful advance. Missing or
    /// stale observations preserve the current transport history without a clock
    /// substitution; all other observation/update faults fence this owner.
    pub fn update_output_clock(
        &mut self,
        host: ClockPoint,
    ) -> Result<Option<DisciplineUpdate>, StepGameplayError> {
        self.ensure_legacy_clock()?;
        self.ensure_usable()?;
        if self.output_clock.is_none() {
            return Ok(None);
        }
        if !self.activated {
            return Err(self.clock_failure(DisciplineError::InvalidConfig));
        }
        if host.domain != self.host_domain {
            return Err(self.clock_failure(DisciplineError::DomainMismatch));
        }
        if self.correction_watermark != Some(host) {
            return Err(self.clock_failure(DisciplineError::NonIncreasing));
        }
        let result = self
            .output_clock
            .as_mut()
            .expect("configured discipline was checked")
            .update(host, self.runtime.transport_mut());
        match result {
            Ok(update) => Ok(Some(update)),
            Err(DisciplineError::NoObservation | DisciplineError::Stale) => Ok(None),
            Err(error) => Err(self.clock_failure(error)),
        }
    }

    fn clock_failure(&mut self, error: DisciplineError) -> StepGameplayError {
        self.failed = true;
        StepGameplayError::Clock(error)
    }

    /// Optionally choose the actual host anchor once resource transfer is done.
    /// This only replaces a pristine Transport; it never resets a judge or
    /// changes the already prepared output-relative BGM and command prefix.
    pub fn activate(&mut self, host_origin: ClockPoint) -> Result<(), StepGameplayError> {
        self.ensure_legacy_clock()?;
        self.ensure_usable()?;
        if self.activated || self.started {
            return Err(StepGameplayError::InvalidConfiguration(
                "activation requires an unactivated, unprocessed runtime",
            ));
        }
        if host_origin.domain != self.host_domain
            || host_origin.timestamp.checked_add(self.preroll).is_none()
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "activation requires the original host domain and representable section start",
            ));
        }
        *self.runtime.transport_mut() = if let Some(setup) = self.live_speed {
            setup
                .transport(host_origin.timestamp)
                .map_err(|error| StepGameplayError::Setup(error.to_string()))?
        } else {
            Transport::new(host_origin.timestamp, self.song, Rate::NORMAL)
        };
        self.activated = true;
        Ok(())
    }

    /// Input provenance, source sequences and clock relations are validated by
    /// the same core path as native solo/cohort play; no retimestamping occurs.
    pub fn process_input(
        &mut self,
        event: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, StepGameplayError> {
        self.process_input_with_position(event, None, mapper, audio_at)
    }

    /// Reports whether input setup can still be admitted without altering gameplay.
    pub fn input_setup_available(&self) -> bool {
        !self.failed && !self.started && !self.activated
    }

    /// Installs spatial contact routing while this contact-mode owner is pristine.
    /// Setup refusal leaves gameplay/capture unchanged and does not fence the owner.
    pub fn configure_touch_router(&mut self, router: TouchRouter) -> Result<(), StepGameplayError> {
        self.ensure_solo()?;
        self.ensure_usable()?;
        if self.started || self.activated || self.input_mode != BmsInputMode::ButtonOrContact {
            return Err(StepGameplayError::InvalidConfiguration(
                "touch routing requires an unactivated, unprocessed contact-mode runtime",
            ));
        }
        self.runtime
            .solo_mut()?
            .configure_touch_router(router)
            .map_err(StepGameplayError::Setup)
    }

    /// Uses a projected hit point while preserving original physical input in
    /// the same runtime reports, score, keysounds and optional replay capture.
    pub fn process_input_at(
        &mut self,
        event: PhysicalInputEvent,
        position: Position2,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, StepGameplayError> {
        self.process_input_with_position(event, Some(position), mapper, audio_at)
    }

    fn process_input_with_position(
        &mut self,
        event: PhysicalInputEvent,
        position: Option<Position2>,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, StepGameplayError> {
        self.ensure_legacy_clock()?;
        self.ensure_solo()?;
        self.ensure_usable()?;
        self.ensure_processing()?;
        self.started = true;
        self.correction_watermark = None;
        let runtime = self.runtime.solo_mut()?;
        let result = match position {
            Some(position) => runtime.process_input_at(event, position, mapper, audio_at),
            None => runtime.process_input(event, mapper, audio_at),
        };
        self.observe(result)
    }

    pub fn advance_to(
        &mut self,
        host: ClockPoint,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<RuntimeReport, StepGameplayError> {
        self.ensure_legacy_clock()?;
        self.ensure_solo()?;
        self.ensure_usable()?;
        self.ensure_processing()?;
        self.started = true;
        self.correction_watermark = None;
        let result = self.runtime.solo_mut()?.advance_to(host, mapper, audio_at);
        let report = self.observe(result)?;
        self.correction_watermark = Some(host);
        self.last_operation = Some(ClockPoint {
            domain: self.logical_domain,
            timestamp: if host.domain == self.logical_domain {
                host.timestamp
            } else {
                mapper
                    .map(host, self.logical_domain)
                    .expect("successful runtime normalized host")
            },
        });
        Ok(report)
    }

    fn observe(
        &mut self,
        result: Result<RuntimeReport, GroupError>,
    ) -> Result<RuntimeReport, StepGameplayError> {
        let was_fenced = self.gameplay_fence().is_some();
        let mut report = match result {
            Ok(report) => report,
            Err(error) => {
                // SoloRuntime returns committed partial reports as Ok. Its
                // pre-report core errors retain the original GroupError here.
                self.failed = true;
                return Err(StepGameplayError::Runtime(error));
            }
        };
        if let Some(input) = &report.input {
            self.last_operation = Some(ClockPoint {
                domain: input.meta().clock_domain,
                timestamp: input.meta().timestamp,
            });
        }
        self.song = report.song_time;
        if !report.audio_commands.is_empty() {
            self.reset_drain();
        }
        let mine_error = self.mine_damage.observe(&report.hazard_events).err();
        let gauge_error = self
            .gauge
            .observe(&report.judge_events, &report.hazard_events)
            .err();
        let score_error = self.score.observe(&report.judge_events).err();
        let capture_error = if was_fenced {
            None
        } else {
            self.capture
                .as_mut()
                .and_then(|capture| capture.record_report(&report).err())
        };
        if self.gauge.snapshot().failure.is_some() {
            let runtime = self.runtime.solo_mut()?;
            runtime.fence_gameplay();
            if let Some(stops) = runtime.fence_gameplay_sounds(report.audio_at.timestamp) {
                if !stops.commands.is_empty() {
                    self.reset_drain();
                }
                report.audio_commands.extend(stops.commands);
                report.audio_failures.extend(stops.failures);
            }
        }
        if let Some(error) = gauge_error {
            self.failed = true;
            return Err(StepGameplayError::Gauge {
                error,
                report,
                score_error,
                capture_error,
                mine_error,
            });
        }
        if let Some(error) = mine_error {
            self.failed = true;
            return Err(StepGameplayError::MineDamage {
                error,
                report,
                score_error,
                capture_error,
            });
        }
        if score_error.is_some()
            || report.judge_error.is_some()
            || !report.audio_failures.is_empty()
        {
            self.failed = true;
            return Err(StepGameplayError::Report {
                report,
                score_error,
                capture_error,
            });
        }
        if let Some(error) = capture_error {
            self.failed = true;
            return Err(StepGameplayError::Capture {
                error,
                report: Some(report),
            });
        }
        Ok(report)
    }

    /// `rendered_frames` is trusted caller-supplied evidence of completed Mixer
    /// rendering on output_origin's grid. Do not pass UI time, a requested arm
    /// frame, admitted commands or an estimated presentation cursor instead.
    pub fn feed_audio(
        &mut self,
        rendered_frames: u64,
        budget: usize,
    ) -> Result<BgmFeedReport, StepGameplayError> {
        self.ensure_usable()?;
        if !(1..=AudioLimits::MAX_COMMANDS).contains(&budget) {
            return Err(StepGameplayError::InvalidConfiguration(
                "BGM budget must be 1..=65536",
            ));
        }
        let previous = self.bgm.report().total_admitted;
        let runtime = &mut self.runtime;
        match self.bgm.feed(rendered_frames, budget, |command| {
            runtime.enqueue_audio(command)
        }) {
            Ok(report) => {
                if report.admitted != 0 {
                    self.reset_drain();
                }
                Ok(report)
            }
            Err(error) => {
                self.failed = true;
                let mut report = self.bgm.report();
                report.admitted = report.total_admitted - previous;
                Err(StepGameplayError::Bgm { error, report })
            }
        }
    }

    /// Observe actual normal-rate Mixer rendering and output presentation.
    /// The caller's mixer must drain at least the entire command queue capacity
    /// in each nonempty render. Buffer length itself may vary. Presentation is
    /// on output_origin's domain; no host time or command ACK substitutes for it.
    pub fn observe_completion(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, StepGameplayError> {
        self.ensure_solo()?;
        if self.retained.is_none()
            && self.live_speed.is_some()
            && self.end.is_some()
            && !self.live_terminal
        {
            self.observe_completion_ready(rendered, presented, false, false)?;
            if let Some(frontier) = self.prepare_live_terminal()? {
                let audio_at = ClockPoint {
                    domain: self.output_origin.domain,
                    timestamp: self
                        .output_origin
                        .timestamp
                        .checked_add(Duration::from_nanos(
                            self.last_presented
                                .expect("terminal presentation checked")
                                .as_nanos(),
                        ))
                        .ok_or(StepGameplayError::InvalidConfiguration(
                            "terminal output point overflow",
                        ))?,
                };
                let result = self
                    .runtime
                    .solo_mut()?
                    .advance_to(frontier, &AudioDomains, audio_at);
                self.observe(result)?;
                self.live_terminal = true;
            }
        }
        let numeric_terminal =
            self.gauge.snapshot().failure.is_some() && self.gameplay_fence().is_some();
        let completed =
            self.observe_completion_ready(rendered, presented, true, numeric_terminal)?;
        if completed && self.completed_result.is_none() {
            self.completed_result = Some(CompletedPlayResult::from_completed(
                self.start,
                self.end,
                &self.gauge,
            ));
        }
        Ok(completed)
    }

    /// Only actual finite execution, retired credits and source presentation
    /// authorize the explicit terminal seek on the existing transport.
    fn prepare_live_terminal(&mut self) -> Result<Option<ClockPoint>, StepGameplayError> {
        let (Some(end), Some(frame), Some(_)) =
            (self.end, self.playback_end_frame, self.live_speed)
        else {
            return Ok(None);
        };
        if let Some(report) = self.last_render {
            // This report already passed the original evidence validator. Use
            // its actual completed playback cursor to retire admitted credits;
            // an ACK or the physical render extent is not a substitute.
            let cursor = completed_render_cursor_with_stops(&report, &self.acknowledged_stops)
                .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
            if let Err(error) = self.bgm.retire_completed(cursor) {
                self.failed = true;
                return Err(StepGameplayError::Bgm {
                    error,
                    report: self.bgm.report(),
                });
            }
        }
        let bgm = self.bgm.report();
        let nanos = (i128::from(frame) * 1_000_000_000 + i128::from(self.sample_rate) - 1)
            / i128::from(self.sample_rate);
        if self.pending.is_some()
            || self.consumer.available_up_to(1) != 0
            || bgm.remaining != 0
            || bgm.outstanding != 0
            || !self.last_render.is_some_and(|report| {
                report.playback_end_physical_frame == Some(frame)
                    && report.counters.commands_consumed == self.acknowledged_commands
                    && report.counters.settled_commands() == Some(self.acknowledged_commands)
                    && report.pending_commands == 0
            })
            || !self
                .last_presented
                .is_some_and(|at| i128::from(at.as_nanos()) >= nanos)
        {
            return Ok(None);
        }
        if self.authority.as_ref().is_some_and(|authority| {
            authority
                .acquired_prefix()
                .is_some_and(|prefix| authority.closed_host_prefix() != Some(prefix))
        }) {
            // An admitted but undrained input prefix still owns original events.
            return Ok(None);
        }
        let anchor = self.runtime.transport_mut().anchor();
        let frontier = self.last_operation.unwrap_or(ClockPoint {
            domain: self.logical_domain,
            timestamp: anchor.host_time,
        });
        // The final point is an already processed logical cutoff (or the actual
        // cold origin for zero-frame playback), never a fabricated future HOST.
        self.runtime
            .transport_mut()
            .seek(frontier.timestamp, end)
            .map_err(|error| StepGameplayError::Setup(error.to_string()))?;
        Ok(Some(frontier))
    }

    fn observe_completion_ready(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
        members_ready: bool,
        numeric_terminal: bool,
    ) -> Result<bool, StepGameplayError> {
        let normalized = self.validate_completion_evidence(rendered, presented)?;
        self.seal_finite_gameplay_audio(rendered);
        let actual = rendered
            .filter(|report| report.frames != 0)
            .or(self.last_render);
        let bgm = self.bgm.report();
        let commands_resolved = self.pending.is_none()
            && self.consumer.available_up_to(1) == 0
            && bgm.remaining == 0
            && bgm.outstanding == 0;
        if let (Some(end), Some(report)) = (self.playback_end_frame, actual) {
            if commands_resolved
                && report.playback_end_physical_frame == Some(end)
                && (report.counters.commands_consumed > self.acknowledged_commands
                    || report
                        .counters
                        .settled_commands()
                        .is_none_or(|settled| settled > self.acknowledged_commands))
            {
                self.failed = true;
                return Err(StepGameplayError::Completion {
                    error: CompletionError(
                        "finite output exceeded or lost exact acknowledged command evidence",
                    ),
                    rendered: Some(report),
                    presented,
                });
            }
        }
        if let Some(report) = rendered.filter(|report| report.frames != 0) {
            self.last_render = Some(report);
        }
        if let Some(point) = normalized {
            self.last_presented = Some(point.timestamp);
        }
        if let Some(state) = &self.retained {
            let terminal = state.ended;
            let cues_finished = state
                .setup
                .cues
                .last()
                .is_none_or(|cue| self.song >= cue.at);
            let ready = state.started
                && !state.paused
                && state.pending.is_none()
                && state.pause.is_none()
                && state.event.is_none()
                && members_ready
                && commands_resolved
                && (terminal || self.end.is_none() && (cues_finished || numeric_terminal))
                && actual.is_some_and(|report| {
                    report.counters.commands_consumed == self.acknowledged_commands
                        && report.counters.settled_commands() == Some(self.acknowledged_commands)
                });
            let completion = self
                .completion
                .as_mut()
                .expect("retained completion is prepared from original assets");
            let report = rendered.or(actual);
            return if terminal || numeric_terminal {
                completion.observe_terminal_ready(ready, bgm, report, normalized)
            } else {
                if !ready {
                    completion.reset_drain();
                    return Ok(false);
                }
                completion.observe(self.runtime.judge(), self.song, bgm, report, normalized)
            }
            .map_err(|error| StepGameplayError::Completion {
                error,
                rendered,
                presented,
            });
        }
        if let (Some(end), Some(frame)) = (self.end, self.playback_end_frame) {
            // The configured endpoint's ceiling timestamp was checked during
            // setup. Presentation remains the real uncapped output observation.
            let nanos = (i128::from(frame) * 1_000_000_000 + i128::from(self.sample_rate) - 1)
                / i128::from(self.sample_rate);
            return Ok(members_ready
                && commands_resolved
                && actual.is_some_and(|report| {
                    report.playback_end_physical_frame == Some(frame)
                        && report.pending_commands == 0
                        && report.counters.commands_consumed == self.acknowledged_commands
                        && report.counters.settled_commands() == Some(self.acknowledged_commands)
                })
                && (self.song == end || numeric_terminal)
                && self
                    .last_presented
                    .is_some_and(|at| i128::from(at.as_nanos()) >= nanos));
        }
        if !members_ready || self.pending.is_some() || self.consumer.available_up_to(1) != 0 {
            self.reset_drain();
            return Ok(false);
        }
        let completion = self
            .completion
            .as_mut()
            .expect("unlimited completion prepared at setup");
        let observed = if numeric_terminal {
            completion.observe_terminal_ready(members_ready, bgm, rendered, normalized)
        } else {
            completion.observe(self.runtime.judge(), self.song, bgm, rendered, normalized)
        };
        match observed {
            Ok(complete) => Ok(complete),
            Err(error) => {
                self.failed = true;
                Err(StepGameplayError::Completion {
                    error,
                    rendered,
                    presented,
                })
            }
        }
    }

    /// Retain validated chronology without evaluating readiness or the drain barrier.
    pub fn admit_output_evidence(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<(), StepGameplayError> {
        let normalized = self.validate_completion_evidence(rendered, presented)?;
        self.seal_finite_gameplay_audio(rendered);
        if let Some(report) = rendered.filter(|report| report.frames != 0) {
            self.last_render = Some(report);
        }
        if let Some(point) = normalized {
            self.last_presented = Some(point.timestamp);
        }
        Ok(())
    }

    /// A validated immutable output endpoint closes sound admission immediately,
    /// while delayed original inputs may still update judge, gauge and capture.
    /// Completion independently retains all ACK, credit and presentation guards.
    fn seal_finite_gameplay_audio(&mut self, rendered: Option<RenderReport>) {
        if self.playback_end_frame.is_some_and(|end| {
            rendered.is_some_and(|report| {
                report.frames != 0 && report.playback_end_physical_frame == Some(end)
            })
        }) {
            self.runtime.seal_gameplay_audio();
        }
    }

    /// Binding preflight before a report is allowed to advance BGM admission.
    /// Successful validation does not adopt the observation or change readiness.
    pub(crate) fn validate_completion_evidence(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<Option<ClockPoint>, StepGameplayError> {
        self.ensure_usable()?;
        let validated = validate_output_evidence_mode(
            self.output_origin,
            self.sample_rate,
            self.last_render,
            self.last_presented,
            rendered,
            presented,
            self.playback_end_frame,
            &self.acknowledged_stops,
            self.retained.is_some(),
        )
        .and_then(|normalized| {
            if self.playback_end_frame.is_some() {
                if let Some(report) = rendered {
                    completed_render_cursor_with_stops(&report, &self.acknowledged_stops).map_err(
                        |_| {
                            CompletionError(
                                "finite output reports a late or rejected audio command",
                            )
                        },
                    )?;
                }
            }
            Ok(normalized)
        });
        match validated {
            Ok(normalized) => Ok(normalized),
            Err(error) => {
                self.failed = true;
                Err(StepGameplayError::Completion {
                    error,
                    rendered,
                    presented,
                })
            }
        }
    }

    /// Drain one bounded batch while retaining its original scalar commands
    /// until ACK. An outstanding batch never pauses the actual judge or producer.
    pub fn take_commands(
        &mut self,
        max: usize,
    ) -> Result<Option<StepAudioBatch>, StepGameplayError> {
        self.ensure_usable()?;
        if !(1..=self.consumer.capacity()).contains(&max) {
            return Err(StepGameplayError::InvalidConfiguration(
                "batch bound must be 1..=command_capacity",
            ));
        }
        if let Some(batch) = &self.pending {
            return Err(StepGameplayError::OutstandingBatch {
                sequence: batch.sequence,
            });
        }
        let count = self
            .consumer
            .available_up_to(if self.playback_end_frame.is_some() {
                self.consumer.capacity()
            } else {
                max
            });
        if count == 0 {
            return Ok(None);
        }
        self.reset_drain();
        let Some(sequence) = self.sequence.checked_add(1) else {
            self.failed = true;
            return Err(StepGameplayError::SequenceOverflow);
        };
        // Reserve both buffers before the first destructive pop. The returned
        // Vec belongs to the adapter; the retained copy records the exact ACK scope.
        let mut commands = Vec::new();
        let mut retained = Vec::new();
        commands
            .try_reserve_exact(count.min(max))
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        retained
            .try_reserve_exact(count.min(max))
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        for _ in 0..count {
            match self.consumer.try_pop() {
                Ok(command) => {
                    match before_endpoint(
                        command.at(),
                        self.output_origin,
                        self.sample_rate,
                        self.playback_end_frame,
                    ) {
                        Ok(false) => continue,
                        Ok(true) => {}
                        Err(error) => {
                            self.failed = true;
                            return Err(StepGameplayError::CommandMapping {
                                error,
                                command,
                                commands,
                            });
                        }
                    }
                    commands.push(command);
                    retained.push(command);
                    if commands.len() == max {
                        break;
                    }
                }
                Err(error) => {
                    self.failed = true;
                    return Err(StepGameplayError::Queue { error, commands });
                }
            }
        }
        if commands.is_empty() {
            return Ok(None);
        }
        self.sequence = sequence;
        self.pending = Some(StepAudioBatch {
            sequence,
            commands: retained,
        });
        Ok(Some(StepAudioBatch { sequence, commands }))
    }

    pub fn acknowledge(
        &mut self,
        sequence: u64,
        admitted: usize,
        success: bool,
    ) -> Result<(), StepGameplayError> {
        self.ensure_usable()?;
        let batch = self.pending.take();
        acknowledge_batch_with_stop_evidence(
            batch,
            sequence,
            admitted,
            success,
            &mut self.acknowledged_stops,
        )
        .map_err(|error| {
            self.failed = true;
            error
        })?;
        if self.playback_end_frame.is_some() || self.retained.is_some() {
            let count = u64::try_from(admitted)
                .ok()
                .and_then(|count| self.acknowledged_commands.checked_add(count));
            let Some(count) = count else {
                self.failed = true;
                return Err(StepGameplayError::AudioCountOverflow { sequence, admitted });
            };
            self.acknowledged_commands = count;
        }
        Ok(())
    }

    /// Fence without inventing completion, a replay operation or a remote ACK.
    pub fn fail(&mut self) {
        self.failed = true;
    }
    pub fn song_time(&self) -> Timestamp {
        self.song
    }
    pub fn end_ns(&self) -> Option<i64> {
        self.end.map(|end| end.as_nanos())
    }
    /// Immutable speed selected for this source play, independently of replay output.
    pub fn playback_speed(&self) -> ReplaySpeed {
        if let Some(state) = &self.retained {
            return state.business.current_speed();
        }
        self.live_speed.map_or_else(
            || ReplaySpeed::new(Rate::NORMAL).expect("positive normal rate"),
            LiveSpeedSetup::speed,
        )
    }
    pub fn playback_end_frame(&self) -> Option<u64> {
        self.playback_end_frame
    }
    pub fn score(&self) -> &ScoreSummary {
        &self.score
    }
    /// Actual committed hazard evidence, also retained after failure.
    pub fn mine_damage(&self) -> &MineDamageSummary {
        &self.mine_damage
    }
    /// Fixed default-policy gauge from actual committed reports, including failures.
    /// Immutable actual stage, gauge and judgment-class selection for this run.
    pub fn play_policy(&self) -> Option<&crate::play_policy::ResolvedPlayPolicy> {
        self.policy.as_ref()
    }
    pub fn gauge(&self) -> &BmsGauge {
        &self.gauge
    }
    /// First proven live completion, retained as history after later errors.
    pub fn completed_result(&self) -> Option<&CompletedPlayResult> {
        self.completed_result.as_ref()
    }
    /// Actual committed failure frontier; later acquisition does not move it.
    pub fn gameplay_fence(&self) -> Option<Timestamp> {
        match &self.runtime {
            RuntimeOwner::Solo(runtime) => runtime.gameplay_fence(),
            RuntimeOwner::Local { group, primary } => group.player_gameplay_fence(*primary),
        }
    }
    pub fn bgm_report(&self) -> BgmFeedReport {
        self.bgm.report()
    }
    /// Actual Stop prefix accepted by valid remote ACKs, retained after failure.
    pub fn acknowledged_stop_commands(&self) -> u64 {
        self.acknowledged_stops.admitted_stops()
    }
    pub fn judge(&self) -> &JudgeEngine {
        self.runtime.judge()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
}

/// Additional failure while observing one member's already committed report.
/// Judge/audio errors remain in the corresponding original PlayerReport.
#[derive(Debug)]
pub struct StepLocalMemberFailure {
    pub player: PlayerId,
    pub score_error: Option<CompetitionError>,
    pub capture_error: Option<CaptureError>,
    pub mine_error: Option<MineDamageError>,
    pub gauge_error: Option<GaugeError>,
}

/// Local failure retains the complete committed group prefix and every member's
/// postprocessing failure. Nothing here rolls back or retries runtime operations.
#[derive(Debug)]
pub enum StepLocalGameplayError {
    Control(StepGameplayError),
    UnknownPlayer(PlayerId),
    Operation {
        group_error: Option<GroupError>,
        reports: Vec<PlayerReport>,
        member_errors: Vec<StepLocalMemberFailure>,
    },
}

impl From<StepGameplayError> for StepLocalGameplayError {
    fn from(error: StepGameplayError) -> Self {
        Self::Control(error)
    }
}

impl fmt::Display for StepLocalGameplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(error) => write!(f, "{error}"),
            Self::UnknownPlayer(player) => write!(f, "unknown local player {}", player.0),
            Self::Operation {
                group_error,
                reports,
                member_errors,
            } => write!(
                f,
                "local operation failed: {group_error:?}; {} committed reports, {} member failures",
                reports.len(),
                member_errors.len(),
            ),
        }
    }
}
impl Error for StepLocalGameplayError {}

struct LocalMemberState {
    player: PlayerId,
    score: ScoreSummary,
    mine_damage: MineDamageSummary,
    gauge: BmsGauge,
    completed_result: Option<CompletedPlayResult>,
    capture: Option<LiveReplayCapture>,
    capture_configured: bool,
    song: Timestamp,
}

/// Independent actual member judges/scores/captures over the same nonblocking
/// audio, clock and completion controller used by StepGameplay. The returned
/// SampleBank stays solely owned by the caller; no PCM is copied for members.
pub struct StepLocalGameplay {
    control: StepGameplay,
    players: Vec<PlayerId>,
    members: Vec<LocalMemberState>,
    objects: Vec<ObjectId>,
    member_error_scratch: Vec<StepLocalMemberFailure>,
}

impl fmt::Debug for StepLocalGameplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepLocalGameplay")
            .field("players", &self.players)
            .field("control", &self.control)
            .finish_non_exhaustive()
    }
}

impl StepLocalGameplay {
    #[allow(clippy::too_many_arguments)]
    pub fn new_retained_audio_section(
        mut prepared: PreparedRetainedSection,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        policy: Option<crate::play_policy::ResolvedPlayPolicy>,
        retained: StepRetainedOptions,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        let setup = prepare_retained_setup(&prepared, config, end, retained, &authority)?;
        let completion = prepare_retained_completion(&mut prepared, config, policy.as_ref())?;
        let start = prepared.section.start;
        let players = plan
            .members()
            .iter()
            .map(|&(player, _)| player)
            .collect::<Vec<_>>();
        let objects = prepared
            .prepared
            .compiled
            .chart
            .objects()
            .iter()
            .map(|object| object.id)
            .collect();
        let (mut control, bank) = StepGameplay::build_mode(
            prepared.prepared,
            config,
            InputSetup::Local(plan, bindings),
            start,
            end,
            input_mode,
            Some(authority),
            policy,
            None,
            true,
        )?;
        control.live_speed = Some(
            LiveSpeedSetup::new(
                start,
                config.preroll,
                ReplaySpeed::new(retained.worklet.initial_rate).map_err(retained_error)?,
                end,
            )
            .map_err(retained_error)?,
        );
        let mut members = Vec::new();
        members
            .try_reserve_exact(players.len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        for &player in &players {
            members.push(LocalMemberState {
                player,
                score: ScoreSummary::default(),
                mine_damage: MineDamageSummary::default(),
                gauge: BmsGauge::new(control.gauge.profile().try_copy().map_err(retained_error)?),
                completed_result: None,
                capture: None,
                capture_configured: false,
                song: control.song,
            });
        }
        let mut owner = Self {
            control,
            players,
            members,
            objects,
            member_error_scratch: Vec::new(),
        };
        owner
            .member_error_scratch
            .try_reserve_exact(owner.players.len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        let mut headers = Vec::new();
        headers
            .try_reserve_exact(owner.players.len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        for &player in &owner.players {
            headers.push(PracticeRuntimeHeader {
                player,
                header: owner.recording_header(
                    player,
                    retained.identity_limits,
                    retained.chart_seed,
                )?,
                limits: retained.identity_limits,
            });
        }
        owner.control.install_retained(
            prepared.original_source,
            retained,
            setup,
            headers,
            completion,
        )?;
        for member in &mut owner.members {
            member.song = owner.control.song;
        }
        Ok((owner, bank))
    }
    pub fn retained_setup(&self) -> Option<&StepRetainedSetup> {
        self.control.retained_setup()
    }
    pub fn configure_retained_limits(
        &mut self,
        limits: beatkernel::audio::PracticeLimits,
        source_history_capacity: usize,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .configure_retained_limits(limits, source_history_capacity)
            .map_err(Into::into)
    }
    pub fn configure_retained_audio_limits(
        &mut self,
        total_voices: usize,
        max_events_per_render: usize,
        control_capacity: usize,
        receipt_capacity: usize,
        source_history_capacity: usize,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .configure_retained_audio_limits(
                total_voices,
                max_events_per_render,
                control_capacity,
                receipt_capacity,
                source_history_capacity,
            )
            .map_err(Into::into)
    }
    pub fn retained_capability(&self) -> Option<PracticeTransactionCapability> {
        self.control.retained_capability()
    }
    pub fn effective_header(&self, player: PlayerId) -> Option<&ReplayHeader> {
        self.control.effective_header(player)
    }
    pub fn prepare_retained_transaction(
        &mut self,
        request: PracticeTransactionRequest,
        fixed_competition: bool,
    ) -> Result<StepRetainedControl, StepLocalGameplayError> {
        self.control.ensure_usable()?;
        let state =
            self.control
                .retained
                .as_mut()
                .ok_or(StepGameplayError::InvalidConfiguration(
                    "retained practice is unavailable",
                ))?;
        let mut captures = self
            .members
            .iter_mut()
            .map(|member| (member.player, &mut member.capture))
            .collect::<Vec<_>>();
        state
            .prepare(
                request,
                fixed_competition,
                self.control.runtime.transport_mut(),
                &mut captures,
            )
            .map_err(Into::into)
    }
    pub fn prepare_retained_pause(
        &mut self,
        id: u64,
        generation: u64,
        paused: bool,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .prepare_retained_pause(id, generation, paused)
            .map_err(Into::into)
    }
    pub fn acknowledge_retained_admission(
        &mut self,
        kind: StepRetainedAdmissionKind,
        id: u64,
        result: Result<(), String>,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .acknowledge_retained_admission(kind, id, result)
            .map_err(Into::into)
    }
    pub fn observe_retained_history(
        &mut self,
        snapshot: StepRemoteHistorySnapshot,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .observe_retained_history(snapshot)
            .map_err(Into::into)
    }
    pub fn retain_output_event(
        &mut self,
        event: WorkletRetainedEnvelope,
    ) -> Result<(), StepLocalGameplayError> {
        self.control.retain_output_event(event).map_err(Into::into)
    }
    pub fn take_retained_response(&mut self) -> Option<PracticeTransactionReply> {
        self.control.take_retained_response()
    }
    pub fn acknowledge_retained_event(
        &mut self,
        serial: u64,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .acknowledge_retained_event(serial)
            .map_err(Into::into)
    }
    pub fn retained_history_pin(&self) -> Result<Option<u64>, StepLocalGameplayError> {
        self.control.retained_history_pin().map_err(Into::into)
    }
    pub fn take_retired_replay(
        &mut self,
        player: PlayerId,
    ) -> Result<Option<Vec<u8>>, StepLocalGameplayError> {
        self.member_index(player)?;
        take_retired_capture(self.control.retained.as_mut(), player).map_err(Into::into)
    }
    pub fn observe_audio_coordinate(
        &mut self,
        epoch: u64,
        pair: ClockPair,
        coordinate: crate::native_output_coordinate::NativeOutputCoordinate,
    ) -> Result<ObservationAdmission, StepLocalGameplayError> {
        self.control
            .observe_audio_coordinate(epoch, pair, coordinate)
            .map_err(Into::into)
    }
    pub fn observe_audio_source_timestamp(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, StepLocalGameplayError> {
        self.control
            .observe_audio_source_timestamp(epoch, pair)
            .map_err(Into::into)
    }
    fn process_next_retained_input(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
        position: Option<Position2>,
    ) -> Result<Option<InputResult>, StepLocalGameplayError> {
        self.control
            .validate_audio_operation(now, audio_at, position)?;
        let Some(prepared) = self.control.retained_input(merger, now, audio_at)? else {
            return Ok(None);
        };
        let state = self.control.retained.as_ref().expect("retained branch");
        let phase = state.phase_point(prepared.output(), prepared.query())?;
        let prefix = self
            .control
            .authority
            .as_ref()
            .expect("audio mode")
            .acquired_prefix()
            .expect("prepared acquired input");
        let position = position.or(merger
            .peek_ready_position(prefix)
            .map_err(AudioAuthorityError::from)
            .map_err(StepGameplayError::from)?);
        let event = merger
            .pop_ready(prefix)
            .map_err(AudioAuthorityError::from)
            .map_err(StepGameplayError::from)?
            .expect("prepared earliest input");
        if state.paused || state.ended {
            let device = event.meta().source;
            self.control
                .authority
                .as_mut()
                .expect("audio mode")
                .commit_coordinate_input(prepared, now)
                .map_err(StepGameplayError::from)?;
            return Ok(Some(InputResult::Ignored { device }));
        }
        let errors = std::mem::take(&mut self.member_error_scratch);
        let result = match position {
            Some(position) => self.group_mut().process_input_at_output_phase(
                event,
                position,
                prepared.mapper(),
                audio_at,
                phase,
            ),
            None => self.group_mut().process_input_output_phase(
                event,
                prepared.mapper(),
                audio_at,
                phase,
            ),
        };
        let committed = match &result {
            Ok(InputResult::Processed(_)) => true,
            Err(error) => !error.completed_reports.is_empty(),
            Ok(InputResult::Ignored { .. }) => false,
        };
        if committed {
            self.control
                .authority
                .as_mut()
                .expect("audio mode")
                .commit_coordinate_input(prepared, now)
                .map_err(StepGameplayError::from)?;
        }
        match result {
            Ok(ignored @ InputResult::Ignored { .. }) => {
                self.member_error_scratch = errors;
                Ok(Some(ignored))
            }
            Ok(InputResult::Processed(reports)) => self
                .finish_reports(Ok(reports), errors)
                .map(InputResult::Processed)
                .map(Some),
            Err(error) => self
                .finish_reports(Err(error), errors)
                .map(InputResult::Processed)
                .map(Some),
        }
    }
    fn advance_retained_frontier(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<Vec<PlayerReport>>, StepLocalGameplayError> {
        self.control.validate_audio_operation(now, audio_at, None)?;
        let state = self.control.retained.as_ref().expect("retained branch");
        if !state.started {
            return Ok(None);
        }
        let authority = self.control.authority.as_mut().expect("audio mode");
        if state.paused || state.ended {
            if let Some(frontier) = authority
                .prepare_coordinate_held_frontier(now, merger)
                .map_err(StepGameplayError::from)?
            {
                authority
                    .commit_coordinate_held_frontier(frontier, now, merger)
                    .map_err(StepGameplayError::from)?;
            }
            return Ok(None);
        }
        let Some(prepared) = authority
            .prepare_coordinate_frontier(now, merger)
            .map_err(StepGameplayError::from)?
        else {
            return Ok(None);
        };
        if !state.permits_query(prepared.query())? {
            return Ok(None);
        }
        let Some(output) = prepared.advance() else {
            authority
                .commit_coordinate_frontier(prepared, now, merger)
                .map_err(StepGameplayError::from)?;
            return Ok(None);
        };
        let phase = state.phase_point(prepared.output(), prepared.query())?;
        let errors = std::mem::take(&mut self.member_error_scratch);
        let result =
            self.group_mut()
                .advance_to_output_phase(output, &AudioDomains, audio_at, phase);
        if result
            .as_ref()
            .map_or_else(|error| !error.completed_reports.is_empty(), |_| true)
        {
            self.control
                .authority
                .as_mut()
                .expect("audio mode")
                .commit_coordinate_frontier(prepared, now, merger)
                .map_err(StepGameplayError::from)?;
            self.control.last_operation = Some(output);
        }
        self.finish_reports(result, errors).map(Some)
    }
    pub fn service_retained_event(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<u64>, StepLocalGameplayError> {
        if let Some((event, true)) = self.control.retained.as_ref().and_then(|state| state.event) {
            return Ok(Some(event.serial));
        }
        let Some((event, cutoff, point)) = self.control.retained_cut(merger, now, audio_at)? else {
            return Ok(None);
        };
        match event.event {
            WorkletRetainedEvent::Practice(projected)
                if !matches!(projected.event, PracticeEvent::Rate(_)) =>
            {
                let update = self
                    .control
                    .retained
                    .as_ref()
                    .expect("retained branch")
                    .boundary_update(projected.event, self.control.end)?;
                if matches!(
                    update.receipt.kind,
                    PracticeBoundaryKind::Looped | PracticeBoundaryKind::Ended
                ) && self
                    .control
                    .runtime
                    .transport_mut()
                    .position_at_output_phase(point)
                    .map_err(retained_error)?
                    != update.region.end
                {
                    return Err(StepGameplayError::InvalidConfiguration(
                        "actual retained endpoint differs from exact active phase",
                    )
                    .into());
                }
                if update.fresh {
                    if !self
                        .control
                        .retained
                        .as_ref()
                        .expect("retained branch")
                        .retired
                        .is_empty()
                    {
                        return Ok(None);
                    }
                    self.install_retained_attempt(&update, cutoff.output())?;
                } else {
                    if update.receipt.kind == PracticeBoundaryKind::Ended {
                        let errors = std::mem::take(&mut self.member_error_scratch);
                        let result = self.group_mut().advance_to_output_phase(
                            cutoff.output(),
                            &AudioDomains,
                            audio_at,
                            point,
                        );
                        self.finish_reports(result, errors)?;
                    }
                    self.control
                        .retained
                        .as_mut()
                        .expect("retained branch")
                        .commit_boundary(&update)?;
                    if update.receipt.kind == PracticeBoundaryKind::Ended {
                        self.control.runtime.seal_gameplay_audio();
                    }
                }
                self.control.started = true;
            }
            _ => {
                let state = self.control.retained.as_mut().expect("retained branch");
                let mut captures = self
                    .members
                    .iter_mut()
                    .map(|member| (member.player, &mut member.capture))
                    .collect::<Vec<_>>();
                state.commit_control(
                    event,
                    point,
                    self.control.runtime.transport_mut(),
                    &mut captures,
                )?;
            }
        }
        self.control
            .authority
            .as_mut()
            .expect("retained authority")
            .commit_coordinate_control_cutoff(cutoff, now, merger)
            .expect("private preflight remains unchanged during cohort commit");
        self.control
            .retained
            .as_mut()
            .expect("retained branch")
            .event = Some((event, true));
        self.control.last_operation = Some(cutoff.output());
        Ok(Some(event.serial))
    }
    fn install_retained_attempt(
        &mut self,
        update: &RetainedBoundaryUpdate,
        logical: ClockPoint,
    ) -> Result<(), StepLocalGameplayError> {
        let state = self.control.retained.as_ref().expect("retained branch");
        let source = crate::section_start::source_at(&state.original, update.region.start)
            .map_err(retained_error)?;
        let mut fresh = Vec::new();
        fresh
            .try_reserve_exact(self.members.len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        for (index, member) in self.members.iter().enumerate() {
            fresh.push(prepare_fresh_step_member(
                state,
                index,
                &source,
                self.judge(member.player)
                    .expect("actual roster")
                    .profile()
                    .clone(),
                member.gauge.profile(),
                self.control.policy.as_ref(),
                self.control.input_mode,
                update,
            )?);
        }
        let basis = OutputPhaseBasis::new(
            self.control.logical_domain,
            state.basis.owner(),
            update.receipt.generation,
            update.receipt.playback_frame,
            self.control.sample_rate,
        )
        .map_err(retained_error)?;
        let phase = PositiveRatePhase::new(
            update.region.start,
            Duration::ZERO,
            update.rate.rate,
            update.end,
        )
        .map_err(retained_error)?;
        let transport = Transport::new_with_runtime_output_phase(
            basis,
            logical,
            update.region.start,
            Duration::ZERO,
            update.rate.rate,
            update.end,
        )
        .map_err(retained_error)?;
        let headers = fresh
            .iter()
            .map(|member| member.header.clone())
            .collect::<Vec<_>>();
        let captured = fresh
            .iter()
            .filter(|member| member.capture.is_some())
            .map(|member| member.player)
            .collect::<Vec<_>>();
        let business = state
            .business
            .candidate_for_attempt(headers.clone(), &captured, phase, update.rate)
            .map_err(retained_error)?;
        let setup = LiveSpeedSetup::new(
            update.region.start,
            Duration::ZERO,
            ReplaySpeed::new(update.rate.rate).map_err(retained_error)?,
            update.end,
        )
        .map_err(retained_error)?;
        let sound_identity = InputSoundIdentity::from_source(&source).map_err(retained_error)?;
        let objects = source
            .compile()
            .map_err(retained_error)?
            .chart
            .objects()
            .iter()
            .map(|object| object.id)
            .collect();
        let completion = state
            .completion_template
            .prepare_practice(&source, &fresh[0].judge)
            .map_err(retained_error)?;
        let original_rate = state.setup.config.initial_rate;
        let mut judges = Vec::new();
        judges
            .try_reserve_exact(fresh.len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        let mut replacements = Vec::new();
        replacements
            .try_reserve_exact(fresh.len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        for member in fresh {
            judges.push((member.player, member.judge));
            replacements.push((member.gauge, member.capture));
        }
        if update.restart {
            self.group_mut()
                .replace_practice_states_with_runtime_restart(
                    judges,
                    transport,
                    update.end,
                    CommandScope(update.receipt.generation),
                    original_rate,
                )
        } else {
            self.group_mut().replace_practice_states_with_output_phase(
                judges,
                transport,
                update.end,
                CommandScope(update.receipt.generation),
            )
        }
        .map_err(retained_error)?;
        let state = self.control.retained.as_mut().expect("retained branch");
        for (member, (gauge, capture)) in self.members.iter_mut().zip(replacements) {
            if let Some(old) = member.capture.take() {
                state.retired.push((member.player, old));
            }
            member.capture = capture;
            member.gauge = gauge;
            member.score = ScoreSummary::default();
            member.mine_damage = MineDamageSummary::default();
            member.completed_result = None;
            member.song = update.region.start;
        }
        state.business = business;
        state.phase = phase;
        state.basis = basis;
        state.headers = headers;
        state
            .commit_boundary(update)
            .expect("fresh candidate already contains the actual revision");
        self.objects = objects;
        self.control.completed_result = None;
        self.control.live_terminal = false;
        self.control.start = update.region.start;
        self.control.end = update.end;
        self.control.preroll = Duration::ZERO;
        self.control.live_speed = Some(setup);
        self.control.input_sound_identity = sound_identity;
        self.control.completion = Some(completion);
        self.control.song = update.region.start;
        Ok(())
    }
    /// Own an already selected chart and its resolved source routes. Even a
    /// one-member local owner uses common local voice remapping; existing solo
    /// constructors retain their original sound IDs and command order.
    pub fn new_section(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared, config, plan, bindings, start, end, input_mode, None, None, None,
        )
    }

    pub fn new_audio_section(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared,
            config,
            plan,
            bindings,
            start,
            end,
            input_mode,
            Some(authority),
            None,
            None,
        )
    }

    /// Every member uses this immutable policy over independent judge/gauge state.
    /// Config offset must agree; policy windows replace config early/late bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn new_section_with_policy(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        policy: crate::play_policy::ResolvedPlayPolicy,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared,
            config,
            plan,
            bindings,
            start,
            end,
            input_mode,
            None,
            Some(policy),
            None,
        )
    }

    /// Selected local stages on the actual logical audio authority timeline.
    #[allow(clippy::too_many_arguments)]
    pub fn new_audio_section_with_policy(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        policy: crate::play_policy::ResolvedPlayPolicy,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared,
            config,
            plan,
            bindings,
            start,
            end,
            input_mode,
            Some(authority),
            Some(policy),
            None,
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_section_with_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared,
            config,
            plan,
            bindings,
            start,
            end,
            input_mode,
            None,
            None,
            Some(speed),
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_audio_section_with_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared,
            config,
            plan,
            bindings,
            start,
            end,
            input_mode,
            Some(authority),
            None,
            Some(speed),
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_section_with_policy_and_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        policy: crate::play_policy::ResolvedPlayPolicy,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared,
            config,
            plan,
            bindings,
            start,
            end,
            input_mode,
            None,
            Some(policy),
            Some(speed),
        )
    }

    /// Cold fixed positive playback speed; input and output clocks keep physical units.
    #[allow(clippy::too_many_arguments)]
    pub fn new_audio_section_with_policy_and_speed(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: AudioAuthority,
        policy: crate::play_policy::ResolvedPlayPolicy,
        speed: ReplaySpeed,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        Self::build(
            prepared,
            config,
            plan,
            bindings,
            start,
            end,
            input_mode,
            Some(authority),
            Some(policy),
            Some(speed),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        prepared: PreparedBms,
        config: StepGameplayConfig,
        plan: ResolvedInputPlan,
        bindings: Vec<BindingMap>,
        start: Timestamp,
        end: Option<Timestamp>,
        input_mode: BmsInputMode,
        authority: Option<AudioAuthority>,
        policy: Option<crate::play_policy::ResolvedPlayPolicy>,
        speed: Option<ReplaySpeed>,
    ) -> Result<(Self, SampleBank), StepLocalGameplayError> {
        let mut players = Vec::new();
        let mut members = Vec::new();
        let mut objects = Vec::new();
        let mut member_error_scratch = Vec::new();
        players
            .try_reserve_exact(plan.members().len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        members
            .try_reserve_exact(plan.members().len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        member_error_scratch
            .try_reserve_exact(plan.members().len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        objects
            .try_reserve_exact(prepared.compiled.chart.objects().len())
            .map_err(|_| StepGameplayError::AllocationFailed)?;
        players.extend(plan.members().iter().map(|&(player, _)| player));
        objects.extend(
            prepared
                .compiled
                .chart
                .objects()
                .iter()
                .map(|object| object.id),
        );
        let (control, bank) = StepGameplay::build(
            prepared,
            config,
            InputSetup::Local(plan, bindings),
            start,
            end,
            input_mode,
            authority,
            policy,
            speed,
        )?;
        for &player in &players {
            members.push(LocalMemberState {
                player,
                score: ScoreSummary::default(),
                mine_damage: MineDamageSummary::default(),
                gauge: BmsGauge::new(
                    control
                        .gauge
                        .profile()
                        .try_copy()
                        .map_err(|error| StepGameplayError::Setup(error.to_string()))?,
                ),
                completed_result: None,
                capture: None,
                capture_configured: false,
                song: control.song,
            });
        }
        Ok((
            Self {
                control,
                players,
                members,
                objects,
                member_error_scratch,
            },
            bank,
        ))
    }

    fn group(&self) -> &RuntimeGroup {
        match &self.control.runtime {
            RuntimeOwner::Local { group, .. } => group,
            RuntimeOwner::Solo(_) => unreachable!("local constructor owns a cohort"),
        }
    }

    fn group_mut(&mut self) -> &mut RuntimeGroup {
        match &mut self.control.runtime {
            RuntimeOwner::Local { group, .. } => group,
            RuntimeOwner::Solo(_) => unreachable!("local constructor owns a cohort"),
        }
    }

    fn member_index(&self, player: PlayerId) -> Result<usize, StepLocalGameplayError> {
        self.players
            .iter()
            .position(|&candidate| candidate == player)
            .ok_or(StepLocalGameplayError::UnknownPlayer(player))
    }

    pub fn players(&self) -> &[PlayerId] {
        &self.players
    }
    pub fn score(&self, player: PlayerId) -> Option<&ScoreSummary> {
        self.members
            .iter()
            .find(|member| member.player == player)
            .map(|member| &member.score)
    }
    /// Independent committed hazard evidence for one actual prepared member.
    pub fn mine_damage(&self, player: PlayerId) -> Option<&MineDamageSummary> {
        self.members
            .iter()
            .find(|member| member.player == player)
            .map(|member| &member.mine_damage)
    }
    /// Independent fixed-policy gauge for one actual prepared member.
    /// Immutable selected policy shared by the independently prepared members.
    pub fn play_policy(&self) -> Option<&crate::play_policy::ResolvedPlayPolicy> {
        self.control.policy.as_ref()
    }
    pub fn gauge(&self, player: PlayerId) -> Option<&BmsGauge> {
        self.members
            .iter()
            .find(|member| member.player == player)
            .map(|member| &member.gauge)
    }
    /// This member's first shared-output completion, also readable after errors.
    /// Unknown identities are distinct from prepared members still awaiting completion.
    pub fn completed_result(
        &self,
        player: PlayerId,
    ) -> Result<Option<&CompletedPlayResult>, StepLocalGameplayError> {
        Ok(self.members[self.member_index(player)?]
            .completed_result
            .as_ref())
    }
    /// Independent committed failure frontier, absent for an unfenced or unknown member.
    pub fn gameplay_fence(&self, player: PlayerId) -> Option<Timestamp> {
        self.group().player_gameplay_fence(player)
    }
    pub fn judge(&self, player: PlayerId) -> Option<&JudgeEngine> {
        self.group().member_judge(player)
    }
    pub fn member_song_time(&self, player: PlayerId) -> Option<Timestamp> {
        self.members
            .iter()
            .find(|member| member.player == player)
            .map(|member| member.song)
    }
    /// Observe the actual ordered cohort without advancing gameplay. Retained
    /// committed prefixes remain readable after failure for final publication.
    /// This bounded control-side allocation is outside input/audio callbacks.
    pub fn group_progress(
        &self,
    ) -> Result<Vec<crate::multiplayer_group::MemberProgress>, StepLocalGameplayError> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(self.members.len()).map_err(|_| {
            StepGameplayError::Setup("local progress snapshot allocation failed".into())
        })?;
        for member in &self.members {
            rows.push(crate::multiplayer_group::MemberProgress {
                player: member.player,
                progress: crate::multiplayer_protocol::Progress {
                    song_ns: member.song.as_nanos(),
                    hits: member.score.hits,
                    misses: member.score.misses,
                    combo: member.score.combo,
                    max_combo: member.score.max_combo,
                },
            });
        }
        Ok(rows)
    }
    pub fn song_time(&self) -> Timestamp {
        self.control.song_time()
    }
    pub fn end_ns(&self) -> Option<i64> {
        self.control.end_ns()
    }
    /// Shared immutable source-play speed for every local member.
    pub fn playback_speed(&self) -> ReplaySpeed {
        self.control.playback_speed()
    }
    pub fn playback_end_frame(&self) -> Option<u64> {
        self.control.playback_end_frame()
    }
    pub fn input_setup_available(&self) -> bool {
        self.control.input_setup_available()
    }
    pub fn failed(&self) -> bool {
        self.control.failed()
    }
    pub fn bgm_report(&self) -> BgmFeedReport {
        self.control.bgm_report()
    }
    /// Actual remote Stop ACK count for the shared queue, not a per-player count.
    /// Remains readable after a technical owner failure.
    pub fn acknowledged_stop_commands(&self) -> u64 {
        self.control.acknowledged_stop_commands()
    }

    pub fn configure_touch_router(
        &mut self,
        player: PlayerId,
        router: TouchRouter,
    ) -> Result<(), StepLocalGameplayError> {
        self.control.ensure_usable()?;
        self.member_index(player)?;
        if !self.input_setup_available() || self.control.input_mode != BmsInputMode::ButtonOrContact
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "touch routing requires an unactivated, unprocessed contact-mode runtime",
            )
            .into());
        }
        self.group_mut()
            .configure_touch_router(player, router)
            .map_err(StepGameplayError::Setup)?;
        Ok(())
    }

    /// Explicit layout changes preserve captures, reports and correction eligibility.
    pub fn remap_touch_regions(
        &mut self,
        player: PlayerId,
        regions: Vec<TouchRegion>,
    ) -> Result<(), StepLocalGameplayError> {
        self.control.ensure_usable()?;
        self.member_index(player)?;
        self.group_mut()
            .remap_touch_regions(player, regions)
            .map_err(StepGameplayError::Setup)?;
        Ok(())
    }

    pub fn set_touch_routing_enabled(
        &mut self,
        player: PlayerId,
        enabled: bool,
    ) -> Result<(), StepLocalGameplayError> {
        self.control.ensure_usable()?;
        self.member_index(player)?;
        self.group_mut()
            .set_touch_routing_enabled(player, enabled)
            .map_err(StepGameplayError::Setup)?;
        Ok(())
    }

    /// Normalized pristine network setup, with the finite end in the identity envelope.
    pub fn competition_header(
        &self,
        player: PlayerId,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<ReplayHeader, StepLocalGameplayError> {
        self.setup_header(player, limits, chart_seed, None)
    }

    /// Capture-exact member metadata for saved comparisons, without enabling capture.
    pub(crate) fn recording_header(
        &self,
        player: PlayerId,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<ReplayHeader, StepLocalGameplayError> {
        self.setup_header(player, limits, chart_seed, self.control.end)
    }

    fn setup_header(
        &self,
        player: PlayerId,
        limits: ReplayCodecLimits,
        chart_seed: u64,
        end: Option<Timestamp>,
    ) -> Result<ReplayHeader, StepLocalGameplayError> {
        self.control.ensure_usable()?;
        self.member_index(player)?;
        if self.control.started {
            return Err(StepGameplayError::InvalidConfiguration(
                "competition identity requires an unprocessed runtime",
            )
            .into());
        }
        if let Some(policy) = &self.control.policy {
            crate::replay_capture::setup_play_policy_header(
                self.judge(player).expect("checked member"),
                self.control.logical_domain,
                limits,
                self.control.start,
                chart_seed,
                end,
                self.control.input_mode,
                self.control.input_sound_identity,
                policy,
            )
        } else {
            setup_gauge_header(
                self.judge(player).expect("checked member"),
                self.control.logical_domain,
                limits,
                self.control.start,
                chart_seed,
                end,
                self.control.input_mode,
                self.control.input_sound_identity,
                self.members[self.member_index(player)?].gauge.profile(),
            )
        }
        .and_then(|header| {
            crate::replay_speed_policy::wrap_header(header, self.control.playback_speed(), limits)
                .map_err(CaptureError::SourceSpeed)
        })
        .map_err(|error| {
            StepGameplayError::Capture {
                error,
                report: None,
            }
            .into()
        })
    }

    pub fn competition_identity(
        &self,
        player: PlayerId,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<Vec<u8>, StepLocalGameplayError> {
        let header = self.competition_header(player, limits, chart_seed)?;
        crate::multiplayer::competition_identity_for_section(
            &header,
            env!("CARGO_PKG_VERSION"),
            limits,
            self.control.end,
        )
        .map_err(|error| StepGameplayError::Setup(error.to_string()).into())
    }

    /// Configures a member's gauge before any member captures or gameplay activates.
    pub fn configure_gauge(
        &mut self,
        player: PlayerId,
        profile: crate::gauge::GaugeProfile,
    ) -> Result<(), StepLocalGameplayError> {
        self.control.ensure_usable()?;
        let index = self.member_index(player)?;
        if !self.input_setup_available()
            || self.members.iter().any(|member| member.capture_configured)
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "gauge configuration requires pristine setup before capture",
            )
            .into());
        }
        if self
            .control
            .policy
            .as_ref()
            .is_some_and(|policy| policy.gauge() != &profile)
        {
            return Err(StepGameplayError::InvalidConfiguration(
                "gauge differs from immutable selected policy",
            )
            .into());
        }
        let replacement = if let Some(state) = &self.control.retained {
            let mut headers = state.headers.clone();
            if self.control.policy.is_none() {
                headers[index].header = setup_gauge_header(
                    self.judge(player).expect("checked member"),
                    self.control.logical_domain,
                    state.options.identity_limits,
                    self.control.start,
                    state.options.chart_seed,
                    self.control.end,
                    self.control.input_mode,
                    self.control.input_sound_identity,
                    &profile,
                )
                .and_then(|header| {
                    crate::replay_speed_policy::wrap_header(
                        header,
                        state.business.current_speed(),
                        state.options.identity_limits,
                    )
                    .map_err(CaptureError::SourceSpeed)
                })
                .map_err(|error| StepGameplayError::Capture {
                    error,
                    report: None,
                })?;
            }
            let business =
                RuntimePractice::new(headers.clone(), &[], state.phase).map_err(retained_error)?;
            Some((headers, business))
        } else {
            None
        };
        self.members[index].gauge = BmsGauge::new(profile);
        if let Some((headers, business)) = replacement {
            let state = self
                .control
                .retained
                .as_mut()
                .expect("retained setup checked");
            state.headers = headers;
            state.business = business;
        }
        Ok(())
    }
    pub fn configure_capture(
        &mut self,
        player: PlayerId,
        limits: ReplayCodecLimits,
        chart_seed: u64,
    ) -> Result<(), StepLocalGameplayError> {
        self.control.ensure_usable()?;
        let index = self.member_index(player)?;
        if self.control.started || self.members[index].capture_configured {
            return Err(StepGameplayError::InvalidConfiguration(
                "capture configuration requires an unprocessed, unconfigured runtime",
            )
            .into());
        }
        let capture = if let Some(policy) = &self.control.policy {
            LiveReplayCapture::new_with_policy_and_speed(
                self.judge(player).expect("checked member"),
                self.control.logical_domain,
                limits,
                self.control.start,
                chart_seed,
                self.control.end,
                self.control.input_mode,
                self.control.input_sound_identity,
                policy,
                self.control.playback_speed(),
            )
        } else {
            LiveReplayCapture::new_with_gauge_and_speed(
                self.judge(player).expect("checked member"),
                self.control.logical_domain,
                limits,
                self.control.start,
                chart_seed,
                self.control.end,
                self.control.input_mode,
                self.control.input_sound_identity,
                self.members[self.member_index(player)?].gauge.profile(),
                self.control.playback_speed(),
            )
        }
        .map_err(|error| StepGameplayError::Capture {
            error,
            report: None,
        })?;
        let business = if let Some(state) = &self.control.retained {
            if state.pending.is_some()
                || state.business.effective_header(player) != Some(capture.header())
            {
                return Err(StepGameplayError::InvalidConfiguration(
                    "capture differs from retained effective identity",
                )
                .into());
            }
            let captured = self
                .members
                .iter()
                .filter(|member| member.capture.is_some() || member.player == player)
                .map(|member| member.player)
                .collect::<Vec<_>>();
            Some(
                RuntimePractice::new(state.headers.clone(), &captured, state.phase)
                    .map_err(retained_error)?,
            )
        } else {
            None
        };
        self.members[index].capture = Some(capture);
        self.members[index].capture_configured = true;
        if let Some(business) = business {
            let state = self
                .control
                .retained
                .as_mut()
                .expect("checked retained mode");
            state.business = business;
            state.capture_limits[index] = Some(limits);
        }
        Ok(())
    }

    /// Export only the whole genuinely completed original roster, without consuming captures.
    pub fn completed_archive(
        &self,
    ) -> Result<Option<Vec<u8>>, crate::result_archive::ArchiveError> {
        self.completed_archive_impl(None)
    }
    pub fn completed_archive_with_comparisons(
        &self,
        rows: &[(
            PlayerId,
            Option<&crate::competition_presentation::CompetitionSnapshot>,
        )],
    ) -> Result<Option<Vec<u8>>, crate::result_archive::ArchiveError> {
        self.completed_archive_impl(Some(rows))
    }
    fn completed_archive_impl(
        &self,
        comparisons: Option<
            &[(
                PlayerId,
                Option<&crate::competition_presentation::CompetitionSnapshot>,
            )],
        >,
    ) -> Result<Option<Vec<u8>>, crate::result_archive::ArchiveError> {
        use crate::result_archive::{encode_archive, ArchiveError, ResultArchive, MAX_PLAYERS};
        if !self.members.iter().any(|member| member.capture_configured) {
            return Ok(None);
        }
        if self
            .members
            .iter()
            .any(|member| member.completed_result.is_none())
        {
            return Ok(None);
        }
        if self.members.is_empty() || self.members.len() > MAX_PLAYERS {
            return Err(ArchiveError::Invalid("completed roster size"));
        }
        let mut results = Vec::new();
        let mut identities = Vec::new();
        let mut scores = Vec::new();
        results
            .try_reserve_exact(self.members.len())
            .map_err(|_| ArchiveError::AllocationFailed)?;
        identities
            .try_reserve_exact(self.members.len())
            .map_err(|_| ArchiveError::AllocationFailed)?;
        scores
            .try_reserve_exact(self.members.len())
            .map_err(|_| ArchiveError::AllocationFailed)?;
        for member in &self.members {
            scores.push((member.player, &member.score));
            let capture = member
                .capture
                .as_ref()
                .ok_or(ArchiveError::Invalid("completed member missing capture"))?;
            identities.push(completed_archive_identity(
                member.player,
                capture,
                member.gauge.profile(),
            )?);
            results.push((
                member.player,
                member
                    .completed_result
                    .ok_or(ArchiveError::Invalid("completed member missing result"))?,
            ));
        }
        let mut archive =
            ResultArchive::from_completed_with_scores(&results, &identities, &scores)?;
        if let Some(rows) = comparisons {
            archive.attach_comparisons(rows)?;
        }
        Ok(Some(encode_archive(&archive)?))
    }

    /// Export each accepted member prefix once after the entire owner is fenced.
    pub fn take_replay(
        &mut self,
        player: PlayerId,
    ) -> Result<Option<Vec<u8>>, StepLocalGameplayError> {
        let index = self.member_index(player)?;
        if !self.control.failed {
            return Err(StepGameplayError::InvalidConfiguration(
                "replay export requires a stopped or fenced runtime",
            )
            .into());
        }
        self.members[index]
            .capture
            .take()
            .map(LiveReplayCapture::into_bytes)
            .transpose()
            .map_err(|error| {
                StepGameplayError::Capture {
                    error,
                    report: None,
                }
                .into()
            })
    }

    pub fn activate_audio(&mut self) -> Result<(), StepLocalGameplayError> {
        self.control.activate_audio().map_err(Into::into)
    }
    pub fn audio_authority(&self) -> Option<&AudioAuthority> {
        self.control.audio_authority()
    }
    pub fn pre_origin_inputs(&self) -> u64 {
        self.control.pre_origin_inputs()
    }
    pub fn discard_pre_origin_audio_inputs(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<PreparedInput>, StepLocalGameplayError> {
        self.control
            .discard_pre_origin_audio_inputs(merger, now, audio_at)
            .map_err(Into::into)
    }
    pub fn observe_audio_output(
        &mut self,
        epoch: u64,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, StepLocalGameplayError> {
        self.control
            .observe_audio_output(epoch, pair)
            .map_err(Into::into)
    }
    pub fn record_audio_prefix(
        &mut self,
        prefix: ClockPoint,
    ) -> Result<(), StepLocalGameplayError> {
        self.control.record_audio_prefix(prefix).map_err(Into::into)
    }
    pub fn process_next_audio_input(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
        position: Option<Position2>,
    ) -> Result<Option<InputResult>, StepLocalGameplayError> {
        if self.control.retained.is_some() {
            return self.process_next_retained_input(merger, now, audio_at, position);
        }
        self.control
            .validate_audio_operation(now, audio_at, position)?;
        let before = self.pre_origin_inputs();
        let Some(prepared) = self.discard_pre_origin_audio_inputs(merger, now, audio_at)? else {
            return Ok(None);
        };
        let position = if before != self.pre_origin_inputs() {
            match self
                .audio_authority()
                .and_then(AudioAuthority::acquired_prefix)
            {
                Some(prefix) => merger
                    .peek_ready_position(prefix)
                    .map_err(AudioAuthorityError::from)
                    .map_err(StepGameplayError::from)?,
                None => None,
            }
        } else {
            position
        };
        self.control
            .validate_audio_operation(now, audio_at, position)?;
        let prefix = self
            .control
            .authority
            .as_ref()
            .expect("audio mode checked")
            .acquired_prefix()
            .expect("prepared input has acquired prefix");
        let event = merger
            .pop_ready(prefix)
            .map_err(AudioAuthorityError::from)
            .map_err(StepGameplayError::from)?
            .expect("private preparation inspected this earliest input");
        let errors = std::mem::take(&mut self.member_error_scratch);
        let result = match position {
            Some(position) => {
                self.group_mut()
                    .process_input_at(event, position, prepared.mapper(), audio_at)
            }
            None => self
                .group_mut()
                .process_input(event, prepared.mapper(), audio_at),
        };
        let committed = match &result {
            Ok(InputResult::Processed(_)) => true,
            Err(error) => !error.completed_reports.is_empty(),
            Ok(InputResult::Ignored { .. }) => false,
        };
        if committed {
            self.control.commit_audio_input(prepared);
        }
        match result {
            Ok(ignored @ InputResult::Ignored { .. }) => {
                self.member_error_scratch = errors;
                Ok(Some(ignored))
            }
            Ok(InputResult::Processed(reports)) => self
                .finish_reports(Ok(reports), errors)
                .map(InputResult::Processed)
                .map(Some),
            Err(error) => self
                .finish_reports(Err(error), errors)
                .map(InputResult::Processed)
                .map(Some),
        }
    }
    pub fn advance_audio_frontier(
        &mut self,
        merger: &mut InputMerger,
        now: ClockPoint,
        audio_at: ClockPoint,
    ) -> Result<Option<Vec<PlayerReport>>, StepLocalGameplayError> {
        if self.control.retained.is_some() {
            return self.advance_retained_frontier(merger, now, audio_at);
        }
        self.control.validate_audio_operation(now, audio_at, None)?;
        let Some(prepared) = self
            .control
            .authority
            .as_ref()
            .expect("audio mode checked")
            .prepare_frontier(now, merger)
            .map_err(StepGameplayError::from)?
        else {
            return Ok(None);
        };
        let Some(output) = prepared.advance() else {
            self.control.commit_audio_frontier(prepared, merger);
            return Ok(None);
        };
        let errors = std::mem::take(&mut self.member_error_scratch);
        let result = self.group_mut().advance_to(output, &AudioDomains, audio_at);
        if result
            .as_ref()
            .map_or_else(|error| !error.completed_reports.is_empty(), |_| true)
        {
            self.control.last_operation = Some(output);
            self.control.commit_audio_frontier(prepared, merger);
        }
        self.finish_reports(result, errors).map(Some)
    }

    pub fn process_input(
        &mut self,
        event: PhysicalInputEvent,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<InputResult, StepLocalGameplayError> {
        self.process_input_with_position(event, None, mapper, audio_at)
    }

    pub fn process_input_at(
        &mut self,
        event: PhysicalInputEvent,
        position: Position2,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<InputResult, StepLocalGameplayError> {
        self.process_input_with_position(event, Some(position), mapper, audio_at)
    }

    fn process_input_with_position(
        &mut self,
        event: PhysicalInputEvent,
        position: Option<Position2>,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<InputResult, StepLocalGameplayError> {
        self.control.ensure_legacy_clock()?;
        self.control.ensure_usable()?;
        self.control.ensure_processing()?;
        let errors = std::mem::take(&mut self.member_error_scratch);
        let result = match position {
            Some(position) => self
                .group_mut()
                .process_input_at(event, position, mapper, audio_at),
            None => self.group_mut().process_input(event, mapper, audio_at),
        };
        match result {
            // Unknown acquisition sources neither lock setup nor invalidate a
            // successfully accepted common clock-correction watermark.
            Ok(ignored @ InputResult::Ignored { .. }) => {
                self.member_error_scratch = errors;
                Ok(ignored)
            }
            Ok(InputResult::Processed(reports)) => self
                .finish_reports(Ok(reports), errors)
                .map(InputResult::Processed),
            Err(error) => self
                .finish_reports(Err(error), errors)
                .map(InputResult::Processed),
        }
    }

    pub fn advance_to(
        &mut self,
        host: ClockPoint,
        mapper: &dyn ClockMapper,
        audio_at: ClockPoint,
    ) -> Result<Vec<PlayerReport>, StepLocalGameplayError> {
        self.control.ensure_legacy_clock()?;
        self.control.ensure_usable()?;
        self.control.ensure_processing()?;
        let errors = std::mem::take(&mut self.member_error_scratch);
        let result = self.group_mut().advance_to(host, mapper, audio_at);
        let reports = self.finish_reports(result, errors)?;
        self.control.correction_watermark = Some(host);
        self.control.last_operation = Some(ClockPoint {
            domain: self.control.logical_domain,
            timestamp: if host.domain == self.control.logical_domain {
                host.timestamp
            } else {
                mapper
                    .map(host, self.control.logical_domain)
                    .expect("successful runtime normalized host")
            },
        });
        Ok(reports)
    }

    fn finish_reports(
        &mut self,
        result: Result<Vec<PlayerReport>, GroupError>,
        mut member_errors: Vec<StepLocalMemberFailure>,
    ) -> Result<Vec<PlayerReport>, StepLocalGameplayError> {
        self.control.started = true;
        self.control.correction_watermark = None;
        let (mut reports, group_error) = match result {
            Ok(reports) => (reports, None),
            Err(error) => (error.completed_reports.clone(), Some(error)),
        };
        let mut reported_failure = false;
        let mut fences = [None; 64];
        for (index, PlayerReport { player, report }) in reports.iter().enumerate() {
            if let Some(input) = &report.input {
                self.control.last_operation = Some(ClockPoint {
                    domain: input.meta().clock_domain,
                    timestamp: input.meta().timestamp,
                });
            }
            let was_fenced = self.gameplay_fence(*player).is_some();
            if !was_fenced {
                self.control.song = report.song_time;
            }
            if !report.audio_commands.is_empty() {
                self.control.reset_drain();
            }
            let member = self
                .members
                .iter_mut()
                .find(|member| member.player == *player)
                .expect("group reports only prepared members");
            member.song = report.song_time;
            let mine_error = member.mine_damage.observe(&report.hazard_events).err();
            let gauge_error = member
                .gauge
                .observe(&report.judge_events, &report.hazard_events)
                .err();
            let score_error = member.score.observe(&report.judge_events).err();
            let capture_error = if was_fenced {
                None
            } else {
                member
                    .capture
                    .as_mut()
                    .and_then(|capture| capture.record_report(report).err())
            };
            if !was_fenced && member.gauge.snapshot().failure.is_some() {
                fences[index] = Some(*player);
            }
            reported_failure |= report.judge_error.is_some() || !report.audio_failures.is_empty();
            if score_error.is_some()
                || capture_error.is_some()
                || mine_error.is_some()
                || gauge_error.is_some()
            {
                member_errors.push(StepLocalMemberFailure {
                    player: *player,
                    score_error,
                    capture_error,
                    mine_error,
                    gauge_error,
                });
            }
        }
        for (index, player) in fences.into_iter().enumerate() {
            let Some(player) = player else {
                continue;
            };
            self.group_mut()
                .fence_player(player)
                .expect("group reports only prepared members");
            let report = &mut reports[index].report;
            if let Some(stops) = self
                .group_mut()
                .fence_player_sounds(player, report.audio_at.timestamp)
                .expect("group reports only prepared members")
            {
                if !stops.commands.is_empty() {
                    self.control.reset_drain();
                }
                reported_failure |= !stops.failures.is_empty();
                report.audio_commands.extend(stops.commands);
                report.audio_failures.extend(stops.failures);
            }
        }
        if group_error.is_some() || reported_failure || !member_errors.is_empty() {
            self.control.fail();
            Err(StepLocalGameplayError::Operation {
                group_error,
                reports,
                member_errors,
            })
        } else {
            self.member_error_scratch = member_errors;
            Ok(reports)
        }
    }

    pub fn configure_output_clock(
        &mut self,
        config: DisciplineConfig,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .configure_output_clock(config)
            .map_err(Into::into)
    }
    pub fn observe_output_clock(
        &mut self,
        pair: ClockPair,
    ) -> Result<ObservationAdmission, StepLocalGameplayError> {
        self.control.observe_output_clock(pair).map_err(Into::into)
    }
    pub fn update_output_clock(
        &mut self,
        host: ClockPoint,
    ) -> Result<Option<DisciplineUpdate>, StepLocalGameplayError> {
        self.control.update_output_clock(host).map_err(Into::into)
    }
    pub fn activate(&mut self, host_origin: ClockPoint) -> Result<(), StepLocalGameplayError> {
        self.control.activate(host_origin).map_err(Into::into)
    }
    /// The rendered cursor has exactly the caller-evidence contract of StepGameplay.
    pub fn feed_audio(
        &mut self,
        rendered_frames: u64,
        budget: usize,
    ) -> Result<BgmFeedReport, StepLocalGameplayError> {
        self.control
            .feed_audio(rendered_frames, budget)
            .map_err(Into::into)
    }
    pub fn take_commands(
        &mut self,
        max: usize,
    ) -> Result<Option<StepAudioBatch>, StepLocalGameplayError> {
        self.control.take_commands(max).map_err(Into::into)
    }
    pub fn acknowledge(
        &mut self,
        sequence: u64,
        admitted: usize,
        success: bool,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .acknowledge(sequence, admitted, success)
            .map_err(Into::into)
    }

    pub fn observe_completion(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<bool, StepLocalGameplayError> {
        self.control.ensure_usable()?;
        if self.control.retained.is_none()
            && self.control.live_speed.is_some()
            && self.control.end.is_some()
            && !self.control.live_terminal
        {
            self.control
                .observe_completion_ready(rendered, presented, false, false)?;
            if let Some(frontier) = self.control.prepare_live_terminal()? {
                let audio_at = ClockPoint {
                    domain: self.control.output_origin.domain,
                    timestamp: self
                        .control
                        .output_origin
                        .timestamp
                        .checked_add(Duration::from_nanos(
                            self.control
                                .last_presented
                                .expect("terminal presentation checked")
                                .as_nanos(),
                        ))
                        .ok_or(StepGameplayError::InvalidConfiguration(
                            "terminal output point overflow",
                        ))?,
                };
                let errors = std::mem::take(&mut self.member_error_scratch);
                let result = self
                    .group_mut()
                    .advance_to(frontier, &AudioDomains, audio_at);
                self.finish_reports(result, errors)?;
                self.control.live_terminal = true;
            }
        }
        let judge_until = self
            .control
            .completion
            .as_ref()
            .map(SongCompletion::judge_until);
        let mut ready = true;
        let mut numeric_failed = false;
        for member in &self.members {
            let failed = member.gauge.snapshot().failure.is_some()
                && self.group().player_gameplay_fence(member.player).is_some();
            numeric_failed |= failed;
            if failed {
                continue;
            }
            let healthy = match self.control.end {
                Some(end) => member.song == end,
                None => {
                    let judge = self.judge(member.player).expect("prepared member");
                    member.song >= judge_until.expect("unlimited completion prepared at setup")
                        && judge.remaining_hazards() == 0
                        && self
                            .objects
                            .iter()
                            .all(|&id| judge.state(id) == Some(InteractionState::Completed))
                }
            };
            ready &= healthy;
        }
        // Admission of real output remains mandatory even while another member
        // is not ready. The shared implementation adopts valid evidence and
        // resets drain readiness without fabricating progress for that member.
        let completed = self.control.observe_completion_ready(
            rendered,
            presented,
            ready,
            numeric_failed && ready,
        )?;
        if completed {
            for member in &mut self.members {
                if member.completed_result.is_none() {
                    member.completed_result = Some(CompletedPlayResult::from_completed(
                        self.control.start,
                        self.control.end,
                        &member.gauge,
                    ));
                }
            }
        }
        Ok(completed)
    }

    pub fn admit_output_evidence(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<(), StepLocalGameplayError> {
        self.control
            .admit_output_evidence(rendered, presented)
            .map_err(Into::into)
    }

    /// Validate genuine output evidence before a browser adapter publishes BGM
    /// commands. Admission and member completion remain the shared owner's job.
    #[cfg(all(target_arch = "wasm32", feature = "browser"))]
    pub(crate) fn validate_completion_evidence(
        &mut self,
        rendered: Option<RenderReport>,
        presented: Option<ClockPoint>,
    ) -> Result<Option<ClockPoint>, StepLocalGameplayError> {
        self.control
            .validate_completion_evidence(rendered, presented)
            .map_err(Into::into)
    }

    pub fn fail(&mut self) {
        self.control.fail();
    }
}

/// Validate and consume exactly one original batch; owners apply their own fence.
pub(crate) fn acknowledge_batch(
    batch: Option<StepAudioBatch>,
    sequence: u64,
    admitted: usize,
    success: bool,
) -> Result<(), StepGameplayError> {
    let valid = batch.as_ref().is_some_and(|batch| {
        sequence == batch.sequence
            && admitted <= batch.commands.len()
            && (!success || admitted == batch.commands.len())
    });
    if !valid {
        return Err(StepGameplayError::InvalidAcknowledgement {
            sequence,
            admitted,
            success,
            batch,
        });
    }
    if !success {
        return Err(StepGameplayError::AudioRejected {
            batch: batch.expect("validated exact batch"),
            admitted,
        });
    }
    Ok(())
}

/// Record only the original prefix accepted by the authoritative ACK validator.
/// Invalid ACKs earn nothing; valid partial rejection retains its original error.
pub(crate) fn acknowledge_batch_with_stop_evidence(
    batch: Option<StepAudioBatch>,
    sequence: u64,
    admitted: usize,
    success: bool,
    evidence: &mut OwnedStopEvidence,
) -> Result<(), StepGameplayError> {
    let mut candidate = *evidence;
    let counted = batch.as_ref().map_or(Ok(()), |batch| {
        candidate.record_admitted(&batch.commands[..admitted.min(batch.commands.len())])
    });
    match acknowledge_batch(batch, sequence, admitted, success) {
        Ok(()) => {
            if counted.is_err() {
                return Err(StepGameplayError::AudioCountOverflow { sequence, admitted });
            }
            *evidence = candidate;
            Ok(())
        }
        Err(error) => {
            if matches!(&error, StepGameplayError::AudioRejected { .. }) && counted.is_ok() {
                *evidence = candidate;
            }
            Err(error)
        }
    }
}

/// Shared validation for normal unlimited live/replay output; no state is adopted.
pub(crate) fn validate_output_evidence(
    output_origin: ClockPoint,
    sample_rate: u32,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    rendered: Option<RenderReport>,
    presented: Option<ClockPoint>,
) -> Result<Option<ClockPoint>, CompletionError> {
    validate_section_output_evidence(
        output_origin,
        sample_rate,
        last_render,
        last_presented,
        rendered,
        presented,
        None,
    )
}

/// Validate original-grid output against an optional immutable relative end.
/// Finite evidence must be nonempty; absent evidence is supplied as `None`.
/// This only validates and normalizes evidence; no completion state is adopted.
pub fn validate_section_output_evidence(
    output_origin: ClockPoint,
    sample_rate: u32,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    rendered: Option<RenderReport>,
    presented: Option<ClockPoint>,
    expected_end: Option<u64>,
) -> Result<Option<ClockPoint>, CompletionError> {
    validate_section_output_evidence_with_stops(
        output_origin,
        sample_rate,
        last_render,
        last_presented,
        rendered,
        presented,
        expected_end,
        &OwnedStopEvidence::default(),
    )
}

/// Shared evidence validation with an actual owner-acknowledged Stop allowance.
pub(crate) fn validate_section_output_evidence_with_stops(
    output_origin: ClockPoint,
    sample_rate: u32,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    rendered: Option<RenderReport>,
    presented: Option<ClockPoint>,
    expected_end: Option<u64>,
    stops: &OwnedStopEvidence,
) -> Result<Option<ClockPoint>, CompletionError> {
    validate_output_evidence_mode(
        output_origin,
        sample_rate,
        last_render,
        last_presented,
        rendered,
        presented,
        expected_end,
        stops,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_output_evidence_mode(
    output_origin: ClockPoint,
    sample_rate: u32,
    last_render: Option<RenderReport>,
    last_presented: Option<Timestamp>,
    rendered: Option<RenderReport>,
    presented: Option<ClockPoint>,
    expected_end: Option<u64>,
    stops: &OwnedStopEvidence,
    retained: bool,
) -> Result<Option<ClockPoint>, CompletionError> {
    if sample_rate == 0 {
        return Err(CompletionError("completion output sample rate is zero"));
    }
    if let Some(end) = expected_end {
        let ns = (i128::from(end) * 1_000_000_000 + i128::from(sample_rate) - 1)
            / i128::from(sample_rate);
        let duration = Duration::from_nanos(
            i64::try_from(ns)
                .map_err(|_| CompletionError("completion endpoint duration overflow"))?,
        );
        output_origin
            .timestamp
            .checked_add(duration)
            .ok_or(CompletionError("completion endpoint clock overflow"))?;
    }
    let normalized = presented
        .map(|point| {
            if point.domain != output_origin.domain {
                return Err(CompletionError(
                    "completion presentation clock domain differs",
                ));
            }
            let ns = point
                .timestamp
                .as_nanos()
                .checked_sub(output_origin.timestamp.as_nanos())
                .ok_or(CompletionError(
                    "completion presentation origin subtraction overflow",
                ))?;
            let timestamp = Timestamp::from_nanos(ns);
            if ns < 0 || last_presented.is_some_and(|last| timestamp < last) {
                return Err(CompletionError(
                    "completion presentation precedes its output frontier",
                ));
            }
            Ok(ClockPoint {
                domain: point.domain,
                timestamp,
            })
        })
        .transpose()?;
    let Some(report) = rendered else {
        return Ok(normalized);
    };
    if report.frames > AudioLimits::MAX_RENDER_FRAMES
        || report.active_voices > AudioLimits::MAX_VOICES
        || report.pending_commands > AudioLimits::MAX_COMMANDS
    {
        return Err(CompletionError(
            "completion render capacity or playback grid differs",
        ));
    }
    let frames = u64::try_from(report.frames)
        .map_err(|_| CompletionError("completion render extent overflow"))?;
    let end = report
        .start_frame
        .checked_add(frames)
        .ok_or(CompletionError("completion render cursor overflow"))?;
    if let Some(endpoint) = expected_end {
        if frames == 0 {
            return Err(CompletionError(
                "finite output evidence requires nonempty render",
            ));
        }
        let playback_frames = frames.min(endpoint.saturating_sub(report.start_frame));
        let marker = (end >= endpoint).then_some(endpoint);
        if report.playback_start_frame != report.start_frame.min(endpoint)
            || report.playback_frames != playback_frames as usize
            || report.paused != marker.is_some()
            || report.playback_end_physical_frame != marker
            || report.producer_disconnected
        {
            return Err(CompletionError(
                "completion output differs from its configured finite playback grid",
            ));
        }
    } else if retained {
        if report.playback_end_physical_frame.is_some()
            || report.producer_disconnected
            || report.playback_start_frame > report.start_frame
            || report.playback_frames != if report.paused { 0 } else { report.frames }
            || report
                .playback_start_frame
                .checked_add(report.playback_frames as u64)
                .is_none_or(|playback| playback > end)
        {
            return Err(CompletionError(
                "retained output differs from its actual pause-excluding grid",
            ));
        }
    } else {
        if report.paused
            || report.playback_end_physical_frame.is_some()
            || report.producer_disconnected
        {
            return Err(CompletionError(
                "completion requires connected unlimited unpaused output",
            ));
        }
        if report.start_frame != report.playback_start_frame
            || report.frames != report.playback_frames
        {
            return Err(CompletionError(
                "completion render capacity or playback grid differs",
            ));
        }
    }
    let end_ns =
        (i128::from(end) * 1_000_000_000 + i128::from(sample_rate) - 1) / i128::from(sample_rate);
    let duration = Duration::from_nanos(
        i64::try_from(end_ns)
            .map_err(|_| CompletionError("completion render duration overflow"))?,
    );
    output_origin
        .timestamp
        .checked_add(duration)
        .ok_or(CompletionError("completion render clock overflow"))?;
    let counters = report.counters;
    if counters.rendered_frames != end
        || counters
            .settled_commands()
            .is_none_or(|settled| settled > counters.commands_consumed)
        || (counters.commands_expired != 0
            && (!report.paused || report.playback_end_physical_frame.is_none()))
        || counters.late_commands > counters.commands_consumed
        || !stops.permits_unknown_stops(counters.unknown_stops)
        || counters.unknown_stops > counters.commands_applied
        || [
            counters.pending_full,
            counters.voice_full,
            counters.unknown_samples,
            counters.invalid_gains,
            counters.invalid_rates,
            counters.invalid_times,
        ]
        .into_iter()
        .any(|value| value != 0)
    {
        return Err(CompletionError(
            "completion mixer counters contain failure or invalid evidence",
        ));
    }
    if let Some(previous) = last_render {
        if retained {
            let previous_playback = previous
                .playback_start_frame
                .checked_add(previous.playback_frames as u64)
                .ok_or(CompletionError("retained playback cursor overflow"))?;
            if report.start_frame != previous.start_frame
                && (report.playback_start_frame < previous_playback
                    || report.start_frame - report.playback_start_frame
                        < previous.start_frame - previous.playback_start_frame
                    || report.start_frame == previous.counters.rendered_frames
                        && report.playback_start_frame != previous_playback)
            {
                return Err(CompletionError(
                    "retained playback or silent-frame history regressed",
                ));
            }
        }
        if report.start_frame == previous.start_frame {
            if report != previous {
                return Err(CompletionError(
                    "completion render block changed after observation",
                ));
            }
        } else if report.start_frame < previous.counters.rendered_frames {
            return Err(CompletionError(
                "completion render chronology regressed or overlapped",
            ));
        }
        if counter_values(counters)
            .into_iter()
            .zip(counter_values(previous.counters))
            .any(|(current, previous)| current < previous)
        {
            return Err(CompletionError("completion mixer counters regressed"));
        }
        if expected_end.is_some()
            && previous.playback_end_physical_frame.is_some()
            && !finite_settlement_progression(previous, report)
        {
            return Err(CompletionError(
                "completion state changed after the finite endpoint",
            ));
        }
    }
    Ok(normalized)
}

pub(crate) fn counter_values(counters: AudioCounters) -> [u64; 12] {
    [
        counters.rendered_frames,
        counters.commands_consumed,
        counters.commands_applied,
        counters.late_commands,
        counters.pending_full,
        counters.voice_full,
        counters.unknown_samples,
        counters.unknown_stops,
        counters.invalid_gains,
        counters.invalid_rates,
        counters.invalid_times,
        counters.commands_expired,
    ]
}

/// After an observed endpoint, only actual consumption and expiry can retire
/// pending debt; execution and frozen playback state cannot change.
pub(crate) fn finite_settlement_progression(previous: RenderReport, report: RenderReport) -> bool {
    let Some(retired_pending) = previous
        .pending_commands
        .checked_sub(report.pending_commands)
        .and_then(|pending| u64::try_from(pending).ok())
    else {
        return false;
    };
    let Some(consumed) = report
        .counters
        .commands_consumed
        .checked_sub(previous.counters.commands_consumed)
    else {
        return false;
    };
    let Some(expired) = report
        .counters
        .commands_expired
        .checked_sub(previous.counters.commands_expired)
    else {
        return false;
    };
    report.playback_end_physical_frame == previous.playback_end_physical_frame
        && report.song_position == previous.song_position
        && report.active_voices == previous.active_voices
        && counter_values(report.counters)[2..11] == counter_values(previous.counters)[2..11]
        && consumed.checked_add(retired_pending) == Some(expired)
}

/// Bounded fallible copies are cold archive work, never part of input/report processing.
fn completed_archive_identity(
    player: PlayerId,
    capture: &LiveReplayCapture,
    profile: &crate::gauge::GaugeProfile,
) -> Result<
    (
        PlayerId,
        beatkernel::replay::ReplayHeader,
        crate::gauge::GaugeProfile,
    ),
    crate::result_archive::ArchiveError,
> {
    use crate::result_archive::{ArchiveError, MAX_HEADER_BYTES};
    let header = capture.header();
    let length = header
        .chart_identity
        .len()
        .checked_add(header.rules_identity.len())
        .and_then(|n| n.checked_add(header.options.len()))
        .ok_or(ArchiveError::TooLarge)?;
    if length > MAX_HEADER_BYTES {
        return Err(ArchiveError::TooLarge);
    }
    let copy = |bytes: &[u8]| -> Result<Vec<u8>, ArchiveError> {
        let mut v = Vec::new();
        v.try_reserve_exact(bytes.len())
            .map_err(|_| ArchiveError::AllocationFailed)?;
        v.extend_from_slice(bytes);
        Ok(v)
    };
    let profile = profile.try_copy().map_err(|error| match error {
        crate::gauge::GaugeError::AllocationFailed => ArchiveError::AllocationFailed,
        _ => ArchiveError::Invalid("gauge profile"),
    })?;
    Ok((
        player,
        beatkernel::replay::ReplayHeader {
            version: header.version,
            chart_identity: copy(&header.chart_identity)?,
            rules_identity: copy(&header.rules_identity)?,
            options: copy(&header.options)?,
            seed: header.seed,
            normalized_clock: header.normalized_clock,
        },
        profile,
    ))
}

#[cfg(test)]
#[path = "step_archived_score_fixtures.rs"]
mod step_archived_score_fixtures;

#[cfg(test)]
#[path = "step_archived_comparison_fixtures.rs"]
mod step_archived_comparison_fixtures;

#[cfg(test)]
#[path = "step_audio_authority_fixtures.rs"]
mod step_audio_authority_fixtures;

#[cfg(test)]
#[path = "step_timing_policy_fixtures.rs"]
mod timing_policy_fixtures;
