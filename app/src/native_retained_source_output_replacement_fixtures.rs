//! Actual retained Source solo/cohort pumps, Runtime, Mixer and encoded capture.
//! Only native effects are scripted; no physical audio timing is claimed.
use crate::gameplay::output::application::replacement::coordinate_source::test_support as support;
use crate::{
    audio_authority::AudioAuthorityConfig,
    bgm::{BgmConfig, BgmFeeder},
    gameplay::output::{
        application::{
            owner::GameplayOutputOwner, replacement::ReplacementState, requests::GameplayOutputUi,
        },
        domain::{
            control::{OutputCapability, OutputReply, OutputRequest},
            remix::matrix_text,
        },
        ports::{
            OutputUiPort, SourcePlaybackIdentity, SourcePlaybackNativeOutputBackend,
            SourceRetirementStatus,
        },
    },
    gameplay_competition::{NoopGroupCompetition, NoopSoloCompetition},
    gameplay_presentation::{GameplayAudioOutputContext, GameplayDevice},
    local_input::InputMerger,
    local_players::PlayerId,
    local_runtime::{MemberConfig, PlayerReport, RuntimeGroup, SoloRuntime},
    native_audio_presentation::NativeAudioPresentation,
    native_audio_startup::new_coordinate_audio_presentation,
    native_cohort::{
        run_cohort_audio_coordinate_practice_with_results_and_ports, AudioCohortSession,
        CohortSession, GameplayPlayerState,
    },
    native_coordinate_control::CoordinatePlaybackControlFacts,
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        run_gameplay_audio_coordinate_with_practice_and_ports, AudioGameplayConfig,
        AudioGameplaySession, CoordinateGameplayConfig, GameplaySession, InputBatch,
        NativeGameplayConfig, NativeGameplayResult,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_playback_phase::NativePlaybackMapping,
    native_pump_control::NativePumpControl,
    play_policy::ResolvedPlayPolicy,
    playback_pause::{NativePause, PausePhase},
    practice_control::{PracticeAction, PracticeCapability, PracticeReply, PracticeRequest},
    practice_playback::{PracticeMember, PracticePlayback, PracticeRecordingPort},
    practice_session::{prepare_attempt_with_speed, PracticeAttemptConfig},
    replay_capture::LiveReplayCapture,
    replay_speed::ReplaySpeed,
    session_launch::SessionLaunch,
    settings::SettingsHost,
};
use beatkernel::{
    audio::*,
    input::*,
    replay::{
        codec::{decode_replay, ReplayCodecLimits},
        ReplayHeader, ReplayOperation, ReplayRecord,
    },
    runtime::{RuntimeProcessingClock, RuntimeReport},
    telemetry::InputDeliveryTelemetry,
    time::{
        presentation::PresentationEstimator, ClockDomainId, ClockPair, ClockPoint, Duration,
        ExtrapolationPolicy, Timestamp,
    },
    transport::{OutputPhaseBasis, Rate},
};
use beatkernel_platform::audio::{DeviceFormat, NativeOutputState, SampleEncoding};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration as WallDuration,
};
use support::{point, Effects, Fault, HistoryFault, Request, Ticket, HOST_START};

#[derive(Debug, PartialEq, Eq)]
struct ColdSource {
    generation: Option<u64>,
    frozen: u64,
    pulled: u64,
    history: support::SourceHistorySnapshot,
}
fn cold_source(output: &support::Output) -> ColdSource {
    let source = output.source_state();
    ColdSource {
        generation: source.mixer().practice_generation(),
        frozen: source.mixer().playback_frame_cursor(),
        pulled: source.mixer().frame_cursor(),
        history: support::source_history(source),
    }
}
#[derive(Clone)]
struct AttemptIdentity {
    player: PlayerId,
    launch: SessionLaunch,
    header: ReplayHeader,
}

type Owner = GameplayOutputOwner<Effects, NativeOutputState, OutputFrameBasis, Ticket>;
const SOURCE_HZ: u32 = 1000;
const EPOCH: u64 = 0;
const CREATION_FRAME: u64 = 5;
const START_FRAME: u64 = CREATION_FRAME + 2;
fn limits() -> ReplayCodecLimits {
    ReplayCodecLimits::new(1_048_576, 4096, 8192, CodecLimits::new(4096, 1024).unwrap()).unwrap()
}
fn policy() -> ResolvedPlayPolicy {
    ResolvedPlayPolicy::builtin(1_000_000, 1_000_000, 0).unwrap()
}
fn end_song() -> Timestamp {
    Timestamp::from_nanos(60_000_000)
}
fn capability(request: &Request) -> OutputCapability {
    let cap = OutputCapability {
        host: SettingsHost::Windows,
        current_args: vec![
            "--backend".into(),
            "wasapi".into(),
            "--mode".into(),
            "shared".into(),
            "--shared-policy".into(),
            "engine".into(),
            "--buffer".into(),
            format!("frames:{}", request.buffer()),
            "--period".into(),
            format!("frames:{}", request.period()),
            "--output-matrix".into(),
            request
                .matrix()
                .map_or_else(|| "exact".into(), |matrix| matrix_text(matrix).unwrap()),
        ],
    };
    cap.validate().unwrap();
    cap
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Race {
    History,
    Query,
    Observation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Bootstrap,
    FirstPaused,
    ReplacingFirst,
    ResumeFirst,
    ActiveAgain,
    SecondPaused,
    RefusingChanged,
    DrainSuffix,
    ReplacingChanged,
    ResumeChanged,
    Loop,
    AutonomousLoop,
    DisableLoop,
    Scrub,
    Finish,
}
struct State {
    stage: Stage,
    desired_pause: bool,
    laps: usize,
    running_laps: usize,
    pause_states: Vec<PauseState>,
    generation: u64,
    generations: Vec<u64>,
    practice_replies: Vec<PracticeReply>,
    practice_sent: Vec<PracticeRequest>,
    identities: Vec<u64>,
    commits: Vec<u64>,
    reports: Vec<(PlayerId, RuntimeReport)>,
    acquired: Vec<PhysicalInputEvent>,
    paused_sequences: Vec<(DeviceId, u64)>,
    suspended_laps: usize,
    old_pending_packets: usize,
    mapping_queries: usize,
    direct_held_admitted: u64,
    history_revisions: Vec<u64>,
    history_authorized_deferrals: usize,
    publication: Vec<(u64, u64, u64, OutputFrameBasis, SourcePlaybackIdentity)>,
    paused_service: Vec<(bool, usize, PausePhase, bool)>,
    ends: Vec<Timestamp>,
    loop_sent: bool,
    disable_sent: bool,
    scrub_sent: bool,
    attempts: Vec<AttemptIdentity>,
    prepared_attempts: Vec<AttemptIdentity>,
    report_starts: Vec<(PlayerId, usize)>,
    archived_attempts: Vec<(PlayerId, u32)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            stage: Stage::Bootstrap,
            desired_pause: false,
            laps: 0,
            running_laps: 0,
            pause_states: vec![],
            generation: 0,
            generations: vec![],
            practice_replies: vec![],
            practice_sent: vec![],
            identities: vec![],
            commits: vec![],
            reports: vec![],
            acquired: vec![],
            paused_sequences: vec![],
            suspended_laps: 0,
            old_pending_packets: 0,
            mapping_queries: 0,
            direct_held_admitted: 0,
            history_revisions: vec![],
            history_authorized_deferrals: 0,
            publication: vec![],
            paused_service: vec![],
            ends: vec![],
            loop_sent: false,
            disable_sent: false,
            scrub_sent: false,
            attempts: vec![],
            prepared_attempts: vec![],
            report_starts: vec![],
            archived_attempts: vec![],
        }
    }
}
#[derive(Default)]
struct Mailbox {
    queue: VecDeque<OutputRequest>,
    mapped: Vec<(u64, Request)>,
    replies: Vec<OutputReply>,
    reply_attempts: Vec<OutputReply>,
    applied: Option<OutputCapability>,
    block_next: bool,
}
struct Port(Rc<RefCell<Mailbox>>);
impl OutputUiPort for Port {
    fn advertise(&mut self, cap: Option<OutputCapability>) -> io::Result<()> {
        self.0.borrow_mut().applied = cap;
        Ok(())
    }
    fn take_request(&mut self) -> io::Result<Option<OutputRequest>> {
        Ok(self.0.borrow_mut().queue.pop_front())
    }
    fn reply(&mut self, reply: &OutputReply) -> io::Result<()> {
        let mut mailbox = self.0.borrow_mut();
        mailbox.reply_attempts.push(reply.clone());
        if mailbox.block_next {
            mailbox.block_next = false;
            return Err(io::ErrorKind::WouldBlock.into());
        }
        assert!(
            !mailbox.replies.iter().any(|old| old.id == reply.id),
            "one final output reply per request"
        );
        if let Ok(cap) = &reply.result {
            mailbox.applied = Some(cap.clone());
        }
        mailbox.replies.push(reply.clone());
        Ok(())
    }
    fn pending(&self) -> bool {
        !self.0.borrow().queue.is_empty()
    }
}
struct Device {
    owner: Owner,
    ui: GameplayOutputUi<Port>,
    effects: Effects,
    mailbox: Rc<RefCell<Mailbox>>,
    state: Rc<RefCell<State>>,
    ids: Vec<u32>,
    sequences: Vec<u64>,
    paused_input_sent: bool,
    changed_request: Request,
    pin: OutputFrameBasis,
    identity: SourcePlaybackIdentity,
    cold_source: Option<ColdSource>,
    history_race_injected: bool,
    query_race_injected: bool,
    observation_race_injected: bool,
    race: Race,
    polls_at_queue: usize,
}
impl Device {
    fn partial_held_prefix(&mut self) {
        let output = self.owner.current_mut().unwrap();
        let before = output.presented;
        output.pump_partial_held(4, 1);
        let admitted = output.presented - before;
        assert_eq!(admitted, 1);
        let report = output.last.unwrap();
        assert!(report.paused && report.playback_frames == 0);
        assert!(output
            .source_state()
            .pending_samples()
            .iter()
            .all(|sample| *sample == 0.0));
        self.state.borrow_mut().direct_held_admitted += admitted;
    }
    fn queue(&mut self, id: u64, request: Request) {
        assert!(self.cold_source.is_none());
        self.cold_source = Some(cold_source(self.owner.current().unwrap()));
        self.polls_at_queue = self
            .effects
            .calls()
            .iter()
            .filter(|call| **call == "conditional-poll")
            .count();
        let mut mailbox = self.mailbox.borrow_mut();
        assert!(mailbox.queue.is_empty());
        mailbox.queue.push_back(OutputRequest {
            id,
            args: capability(&request).current_args,
        });
        mailbox.mapped.push((id, request));
    }
    fn input(&mut self, events: &mut VecDeque<PhysicalInputEvent>, paused: bool) {
        let at = self.effects.pump_now();
        for (index, &id) in self.ids.iter().enumerate() {
            for button in [ButtonState::Down, ButtonState::Up] {
                self.sequences[index] += 1;
                let event = PhysicalInputEvent::Button(ButtonEvent {
                    meta: EventMeta::new(DeviceId(u64::from(id)), at, self.sequences[index]),
                    control: PhysicalControlId::keyboard(7),
                    state: button,
                });
                let mut state = self.state.borrow_mut();
                state.acquired.push(event.clone());
                if paused {
                    state
                        .paused_sequences
                        .push((event.meta().source, event.meta().sequence));
                }
                events.push_back(event);
            }
        }
    }
}
impl GameplayDevice for Device {
    type Presentation = PresentationEstimator;
    fn request_history_retirement(
        &mut self,
        presentation: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
        source_minimum: Option<u64>,
    ) -> NativeGameplayResult<Option<u64>> {
        let inject = self.race == Race::History
            && !self.history_race_injected
            && self.owner.state() == ReplacementState::ConditionalRetirement
            && self.state.borrow().stage == Stage::ReplacingFirst
            && self.state.borrow().old_pending_packets > 0;
        if inject {
            self.effects
                .history_request_faults([HistoryFault::Authorize]);
        }
        let query_inject = self.race == Race::Query
            && !self.query_race_injected
            && self.owner.state() == ReplacementState::ConditionalRetirement
            && self.state.borrow().stage == Stage::ReplacingFirst
            && self.state.borrow().old_pending_packets > 0;
        let query_count = self.effects.history_queries().len();
        let request_count = self.effects.history_requests().len();
        if query_inject {
            self.effects.history_query_faults([HistoryFault::Authorize]);
        }
        let revision = self
            .owner
            .request_conditional_source_history_retirement_with_source_pin(
                presentation,
                query,
                identity,
                source_minimum,
            )?;
        if inject {
            assert!(revision.is_none());
            assert!(self.owner.current().is_none());
            assert!(self.owner.output_clock_suspended());
            self.history_race_injected = true;
            self.state.borrow_mut().history_authorized_deferrals += 1;
        }
        if query_inject {
            assert_eq!(self.effects.history_queries().len(), query_count + 1);
            assert_eq!(self.effects.history_requests().len(), request_count);
            assert!(revision.is_none());
            assert!(self.owner.current().is_none());
            assert!(self.owner.output_clock_suspended());
            self.query_race_injected = true;
        }
        if let Some(revision) = revision {
            self.state.borrow_mut().history_revisions.push(revision);
        }
        Ok(revision)
    }
    fn observe_coordinate_playback_control(
        &mut self,
        presentation: &mut NativeAudioPresentation,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<CoordinatePlaybackControlFacts>> {
        self.state.borrow_mut().laps += 1;
        if self.state.borrow().laps > 240 {
            return Err(format!(
                "connected pump did not finish: {:?}",
                self.state.borrow().stage
            )
            .into());
        }
        let was_pending = self.owner.replacement_pending();
        let polls = self
            .effects
            .calls()
            .iter()
            .filter(|call| **call == "conditional-poll")
            .count();
        let inject = self.race == Race::Observation
            && !self.observation_race_injected
            && self.owner.state() == ReplacementState::ConditionalRetirement
            && self.state.borrow().stage == Stage::ReplacingFirst
            && polls == self.polls_at_queue + 1;
        if inject {
            self.effects.observation_faults([Fault::Observe]);
        }
        let packet = self
            .owner
            .observe_conditional_source_native_control(presentation, identity)?;
        if inject {
            assert!(packet.is_none());
            assert!(self.owner.output_clock_suspended());
            self.observation_race_injected = true;
        }
        if was_pending && packet.is_some() {
            self.state.borrow_mut().old_pending_packets += 1;
        }
        if self.owner.output_clock_suspended() {
            self.state.borrow_mut().suspended_laps += 1;
        }
        Ok(packet.map(CoordinatePlaybackControlFacts::Source))
    }
    fn playback_mapping_at(
        &self,
        _: &NativeAudioPresentation,
        _: &InterpolatedTargetTime,
        _: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        panic!("Source common pumps must use their mutable conditional query hook")
    }
    fn conditional_playback_mapping_at(
        &mut self,
        presentation: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        self.state.borrow_mut().mapping_queries += 1;
        let packet = self
            .owner
            .conditional_source_playback_at(presentation, query, identity)?;
        Ok(packet.as_ref().map(NativePlaybackMapping::from_source))
    }
    fn output_clock_suspended(&self) -> bool {
        self.owner.output_clock_suspended()
    }
    fn output_replacement_pending(&self) -> bool {
        self.owner.has_work() || self.ui.pending()
    }
    fn publish_paused_coordinate_audio_output(
        &mut self,
        context: GameplayAudioOutputContext<'_>,
        now: ClockPoint,
        identity: SourcePlaybackIdentity,
        practice_pending: bool,
    ) -> NativeGameplayResult<bool> {
        assert_eq!(context.pause.phase(), PausePhase::Paused);
        assert_eq!(context.presentation.practice_mixer_basis(), Some(self.pin));
        assert_eq!(identity, self.identity);
        assert!(context.end.is_none());
        self.state.borrow_mut().paused_service.push((
            self.owner.output_clock_suspended(),
            context.merger.pending(),
            context.pause.phase(),
            practice_pending,
        ));
        let stage = self.state.borrow().stage;
        match stage {
            Stage::FirstPaused => {
                self.partial_held_prefix();
                self.effects.pump_admit_limit(1);
                if self.race == Race::Observation {
                    self.effects.conditional_statuses([
                        SourceRetirementStatus::Pending,
                        SourceRetirementStatus::Pending,
                        SourceRetirementStatus::Authorized,
                    ]);
                } else {
                    self.effects
                        .conditional_statuses([SourceRetirementStatus::Pending; 32]);
                    self.effects.observation_faults([
                        Fault::Missing,
                        Fault::Missing,
                        Fault::None,
                        Fault::Missing,
                        Fault::Missing,
                    ]);
                }
                self.mailbox.borrow_mut().block_next = true;
                self.queue(101, Request::normal(self.changed_request.frequency));
                let mut state = self.state.borrow_mut();
                state.stage = Stage::ReplacingFirst;
                state.desired_pause = false;
            }
            Stage::SecondPaused => {
                self.partial_held_prefix();
                assert!(
                    self.owner
                        .current()
                        .unwrap()
                        .source_state()
                        .validate_reconfigure(
                            self.changed_request.format(),
                            self.changed_request.matrix(),
                            self.changed_request.buffer() as usize
                        )
                        .is_err(),
                    "changed channel layout cannot flatten the original partial suffix"
                );
                self.effects.pump_admit_limit(1);
                self.effects
                    .conditional_statuses([SourceRetirementStatus::Refused]);
                // A native refusal is a terminal decision on an otherwise valid
                // same-format request; it must preserve this exact live owner.
                let mut refused = Request::normal(self.changed_request.frequency);
                refused.period = 16;
                self.queue(102, refused);
                self.state.borrow_mut().stage = Stage::RefusingChanged;
            }
            Stage::DrainSuffix
                if self
                    .owner
                    .current()
                    .unwrap()
                    .source_state()
                    .pending_frames()
                    == 0 =>
            {
                assert!(self
                    .owner
                    .current()
                    .unwrap()
                    .source_state()
                    .validate_reconfigure(
                        self.changed_request.format(),
                        self.changed_request.matrix(),
                        self.changed_request.buffer() as usize
                    )
                    .is_ok());
                self.effects.pump_admit_limit(usize::MAX);
                self.effects
                    .conditional_statuses([SourceRetirementStatus::Authorized]);
                self.queue(103, self.changed_request.clone());
                self.state.borrow_mut().stage = Stage::ReplacingChanged;
            }
            _ => {}
        }
        let before_epoch = context.presentation.authority().epoch().id;
        let generation = self.state.borrow().generation;
        let mapped = self.mailbox.clone();
        let published = self.ui.service_coordinate_source_audio(
            &mut self.owner,
            context,
            now,
            identity,
            practice_pending,
            &mut |request, _| {
                mapped
                    .borrow()
                    .mapped
                    .iter()
                    .find(|(id, _)| *id == request.id)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| "unmapped actual request".into())
            },
            &mut |output| Ok(capability(output.applied_request())),
        )?;
        if published {
            let output = self.owner.current().unwrap();
            let before = self.cold_source.take().expect("actual cold original owner");
            let after = cold_source(output);
            assert_eq!(after.generation, before.generation);
            assert_eq!(after.frozen, before.frozen);
            assert!(
                after.pulled > before.pulled,
                "new native proof requires fresh held rendering"
            );
            assert_eq!(
                after.history.initial_physical,
                before.history.initial_physical
            );
            assert_eq!(
                after.history.initial_playback,
                before.history.initial_playback
            );
            assert_eq!(after.history.spans, before.history.spans);
            assert_eq!(
                after.history.retirement_revision,
                before.history.retirement_revision
            );
            assert_eq!(
                output.source_state().mixer().applied_start_frame(),
                Some(START_FRAME)
            );
            assert!(
                self.state.borrow().archived_attempts.is_empty(),
                "output replacement itself never archives an attempt"
            );
            assert!(output.source_state().mixer().is_paused());
            assert_eq!(
                self.state.borrow().generation,
                generation,
                "output change creates no attempt"
            );
            self.state.borrow_mut().publication.push((
                before_epoch,
                output.creation_epoch(),
                generation,
                self.pin,
                identity,
            ));
        }
        let replies = self.mailbox.borrow().replies.clone();
        let stage = self.state.borrow().stage;
        if stage == Stage::ReplacingFirst && replies.iter().any(|reply| reply.id == 101) {
            assert!(replies
                .iter()
                .find(|reply| reply.id == 101)
                .unwrap()
                .result
                .is_ok());
            self.state.borrow_mut().stage = Stage::ResumeFirst;
        } else if stage == Stage::RefusingChanged && replies.iter().any(|reply| reply.id == 102) {
            assert!(replies
                .iter()
                .find(|reply| reply.id == 102)
                .unwrap()
                .result
                .is_err());
            assert_eq!(self.owner.current().unwrap().creation_epoch(), EPOCH + 1);
            assert_eq!(
                cold_source(self.owner.current().unwrap()),
                self.cold_source
                    .take()
                    .expect("live refusal retains the cold owner")
            );
            self.state.borrow_mut().stage = Stage::DrainSuffix;
        } else if stage == Stage::ReplacingChanged && replies.iter().any(|reply| reply.id == 103) {
            assert!(replies
                .iter()
                .find(|reply| reply.id == 103)
                .unwrap()
                .result
                .is_ok());
            let mut state = self.state.borrow_mut();
            state.stage = Stage::ResumeChanged;
            state.desired_pause = false;
        }
        Ok(published)
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        if self.owner.output_clock_suspended() && !self.paused_input_sent {
            self.paused_input_sent = true;
            self.input(events, true);
        } else if self.state.borrow().pause_states.last() == Some(&PauseState::Running)
            && self.state.borrow().laps % 3 == 0
        {
            self.input(events, false);
        }
        Ok(InputBatch {
            completed_through: Some(self.effects.pump_now()),
            backlog: false,
            closed: false,
        })
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        Ok(self.owner.render_report())
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        Ok(self.effects.pump_now())
    }
    fn observe(&mut self, _: &mut Self::Presentation) -> NativeGameplayResult<()> {
        panic!("scalar observer")
    }
    fn observe_audio(&mut self, _: &mut NativeAudioPresentation) -> NativeGameplayResult<()> {
        panic!("coordinate control must own native observation")
    }
    fn observe_end(
        &mut self,
        _: &mut NativeEnd,
        _: &Self::Presentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        panic!("scalar end")
    }
    fn seed_resume(
        &mut self,
        _: &mut Self::Presentation,
        _: ClockPair,
    ) -> NativeGameplayResult<()> {
        panic!("scalar resume")
    }
    fn fallback_schedule(&mut self, _: u32) -> NativeGameplayResult<ClockPoint> {
        panic!("scalar schedule")
    }
}
struct Host(Rc<RefCell<State>>);
impl NativeGameplayHost for Host {
    fn prepare_policies(
        &mut self,
        _: &[(PlayerId, &crate::gauge::GaugeProfile)],
    ) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        let mut state = self.0.borrow_mut();
        if state.stage == Stage::Bootstrap && state.laps >= 3 && !state.generations.is_empty() {
            state.desired_pause = true;
        }
        if state.stage == Stage::ActiveAgain {
            state.running_laps += 1;
            if state.running_laps >= 3 {
                state.desired_pause = true;
            }
        }
        state.desired_pause
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, pause: PauseState) {
        let mut state = self.0.borrow_mut();
        state.pause_states.push(pause);
        match (state.stage, pause) {
            (Stage::Bootstrap, PauseState::Paused) => state.stage = Stage::FirstPaused,
            (Stage::ResumeFirst, PauseState::Running) => state.stage = Stage::ActiveAgain,
            (Stage::ActiveAgain, PauseState::Paused) => state.stage = Stage::SecondPaused,
            (Stage::ResumeChanged, PauseState::Running) => state.stage = Stage::Loop,
            _ => {}
        }
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        self.0.borrow_mut().ends.push(end);
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.0
            .borrow_mut()
            .reports
            .push((PlayerId(1), report.clone()));
        Ok(())
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.0.borrow_mut().reports.extend(
            reports
                .iter()
                .map(|report| (report.player, report.report.clone())),
        );
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
    fn advertise_practice(
        &mut self,
        capability: Option<PracticeCapability>,
    ) -> NativeGameplayResult<()> {
        if let Some(cap) = capability {
            let mut state = self.0.borrow_mut();
            state.generation = cap.generation;
            state.generations.push(cap.generation);
            if state.stage == Stage::AutonomousLoop && cap.generation >= 3 {
                state.stage = Stage::DisableLoop;
            }
        }
        Ok(())
    }
    fn take_practice_request(&mut self) -> NativeGameplayResult<Option<PracticeRequest>> {
        let mut state = self.0.borrow_mut();
        let action = match state.stage {
            Stage::Loop if !state.loop_sent => {
                state.loop_sent = true;
                PracticeAction::Loop {
                    start: Timestamp::from_nanos(2_000_000),
                    end: Timestamp::from_nanos(18_000_000),
                }
            }
            Stage::DisableLoop if !state.disable_sent => {
                state.disable_sent = true;
                PracticeAction::DisableLoop
            }
            Stage::Scrub if !state.scrub_sent => {
                state.scrub_sent = true;
                PracticeAction::Scrub {
                    target: Timestamp::from_nanos(2_000_001),
                }
            }
            _ => return Ok(None),
        };
        let request = PracticeRequest {
            id: 201 + state.practice_sent.len() as u64,
            generation: state.generation,
            action,
        };
        state.practice_sent.push(request);
        Ok(Some(request))
    }
    fn commit_practice_reply(&mut self, reply: &PracticeReply) -> NativeGameplayResult<()> {
        assert!(
            reply.result.is_ok(),
            "real practice request refused: {reply:?}"
        );
        let mut state = self.0.borrow_mut();
        state.practice_replies.push(reply.clone());
        state.stage = match state.stage {
            Stage::Loop => Stage::AutonomousLoop,
            Stage::DisableLoop => Stage::Scrub,
            Stage::Scrub => Stage::Finish,
            other => other,
        };
        Ok(())
    }
    fn prepare_practice_presentation(
        &mut self,
        generation: u64,
        attempts: &[(PlayerId, &crate::practice_session::PreparedPracticeAttempt)],
    ) -> NativeGameplayResult<crate::player::PreparedPracticePresentation> {
        self.0.borrow_mut().prepared_attempts = attempts
            .iter()
            .map(|(player, attempt)| AttemptIdentity {
                player: *player,
                launch: attempt.next_launch.clone(),
                header: attempt
                    .capture
                    .as_ref()
                    .expect("actual fresh member capture")
                    .header()
                    .clone(),
            })
            .collect();
        crate::player::prepare_practice_presentation(generation, attempts)
    }
    fn apply_practice_identity(
        &mut self,
        prepared: &mut crate::player::PreparedPracticePresentation,
        generation: u64,
    ) {
        crate::player::apply_practice_identity(prepared, generation);
        let mut state = self.0.borrow_mut();
        let prepared = std::mem::take(&mut state.prepared_attempts);
        for next in &prepared {
            let old = state
                .attempts
                .iter()
                .find(|old| old.player == next.player)
                .unwrap();
            assert_eq!(next.launch.attempt(), old.launch.attempt() + 1);
        }
        state.attempts = prepared;
        state.identities.push(generation);
    }
    fn commit_practice_presentation(
        &mut self,
        prepared: crate::player::PreparedPracticePresentation,
        generation: u64,
    ) -> NativeGameplayResult<()> {
        crate::player::commit_practice_presentation(prepared, generation)?;
        self.0.borrow_mut().commits.push(generation);
        Ok(())
    }
}
struct Control {
    effects: Effects,
    ticks: usize,
}
impl NativePumpControl for Control {
    type Moment = usize;
    fn now(&mut self) -> NativeGameplayResult<usize> {
        Ok(self.ticks)
    }
    fn checked_add(moment: usize, _: WallDuration) -> Option<usize> {
        moment.checked_add(1)
    }
    fn wait(&mut self, _: WallDuration) -> NativeGameplayResult<()> {
        self.ticks += 1;
        self.effects.pump_wait();
        Ok(())
    }
}
struct RecordedAttempt {
    player: PlayerId,
    path: PathBuf,
    bytes: Vec<u8>,
    identity: AttemptIdentity,
    accepted: Vec<ReplayRecord>,
    events: Vec<beatkernel::judge::JudgeEvent>,
    live_hash: Option<u64>,
}
struct Recordings {
    files: Vec<RecordedAttempt>,
    state: Rc<RefCell<State>>,
}
impl Recordings {
    fn encoded_attempt(
        &mut self,
        player: PlayerId,
        path: &Path,
        capture: LiveReplayCapture,
        live_hash: Option<u64>,
    ) -> NativeGameplayResult<RecordedAttempt> {
        let mut state = self.state.borrow_mut();
        let identity = state
            .attempts
            .iter()
            .find(|attempt| attempt.player == player)
            .expect("actual active attempt identity")
            .clone();
        let expected_path = identity
            .launch
            .args()
            .chunks_exact(2)
            .find(|pair| pair[0] == "--record-replay")
            .unwrap();
        assert_eq!(path, Path::new(&expected_path[1]));
        assert_eq!(capture.header(), &identity.header);
        let start = state
            .report_starts
            .iter()
            .find(|entry| entry.0 == player)
            .map_or(0, |entry| entry.1);
        let mut accepted = Vec::new();
        let mut events = Vec::new();
        for (_, report) in state.reports[start..]
            .iter()
            .filter(|(id, _)| *id == player)
        {
            assert!(report.judge_error.is_none() && report.audio_failures.is_empty());
            for input in &report.bound_inputs {
                accepted.push(ReplayRecord {
                    ordinal: accepted.len() as u64,
                    song_time: report.song_time,
                    operation: ReplayOperation::Input(input.clone()),
                });
            }
            if report.input.is_none() {
                accepted.push(ReplayRecord {
                    ordinal: accepted.len() as u64,
                    song_time: report.song_time,
                    operation: ReplayOperation::Advance,
                });
            }
            events.extend(report.judge_events.iter().cloned());
        }
        assert_eq!(
            capture.records(),
            accepted.as_slice(),
            "actual captured prefix equals the independently published accepted operations"
        );
        let end = state.reports.len();
        if let Some(entry) = state
            .report_starts
            .iter_mut()
            .find(|entry| entry.0 == player)
        {
            entry.1 = end;
        } else {
            state.report_starts.push((player, end));
        }
        if live_hash.is_none() {
            assert!(
                !state
                    .archived_attempts
                    .contains(&(player, identity.launch.attempt())),
                "one archive per original member attempt"
            );
            state
                .archived_attempts
                .push((player, identity.launch.attempt()));
        }
        Ok(RecordedAttempt {
            player,
            path: path.to_owned(),
            bytes: capture.into_bytes()?,
            identity,
            accepted,
            events,
            live_hash,
        })
    }
}
impl PracticeRecordingPort for Recordings {
    fn archive(
        &mut self,
        player: PlayerId,
        path: &Path,
        capture: LiveReplayCapture,
    ) -> NativeGameplayResult<()> {
        let file = self.encoded_attempt(player, path, capture, None)?;
        self.files.push(file);
        Ok(())
    }
}
enum Runtimes {
    Solo {
        runtime: SoloRuntime,
        gauge: crate::gauge::BmsGauge,
        capture: Option<LiveReplayCapture>,
    },
    Cohort {
        group: RuntimeGroup,
        states: Vec<GameplayPlayerState<NoopSoloCompetition>>,
    },
}
struct Fixture {
    original: beatkernel_bms::BmsChart,
    runtimes: Runtimes,
    device: Device,
    presentation: NativeAudioPresentation,
    pause: NativePause,
    merger: InputMerger,
    bgm: BgmFeeder,
    playback: PracticePlayback,
    recordings: Recordings,
    host: Host,
    control: Control,
    config: CoordinateGameplayConfig,
}
impl Fixture {
    fn new(ids: &[u32], cohort: bool, frequency: Option<u64>, race: Race) -> Self {
        assert!(!cohort || ids.len() >= 2);
        let original = beatkernel_bms::parse(
            "#BPM 3000\n#WAV01 key.wav\n#00011:0001000000010000\n",
            Default::default(),
        )
        .unwrap();
        let rate = Rate::new(3, 2).unwrap();
        let speed = ReplaySpeed::new(rate).unwrap();
        let setup = crate::live_speed::LiveSpeedSetup::new(
            Timestamp::ZERO,
            Duration::ZERO,
            speed,
            Some(end_song()),
        )
        .unwrap();
        let format = AudioFormat::new(SOURCE_HZ, 1).unwrap();
        let pcm_limits = PcmLimits::new(4096, 8192, 2).unwrap();
        let mut bank = SampleBank::new(format, pcm_limits).unwrap();
        bank.insert(
            SampleId(1),
            PcmSample::new(format, vec![0.25; 1024], pcm_limits).unwrap(),
        )
        .unwrap();
        let program = PreparedPracticeProgram::new_with_speed(
            &bank,
            vec![PracticeCue {
                voice: VoiceId(1),
                sample: SampleId(1),
                at: Timestamp::ZERO,
                gain: 1.0,
            }],
            PracticeLimits::new(8, 2, 128, 16, 512).unwrap(),
            rate,
            Duration::ZERO,
            false,
        )
        .unwrap();
        let (controller, endpoint) = practice_queue(&program).unwrap();
        let (mut producer, consumer) = command_queue_with_start_gate(256).unwrap();
        producer.schedule_start_at(START_FRAME).unwrap();
        let mut mixer = Mixer::new(
            MixerConfig::new(
                format,
                ClockDomainId(2),
                Timestamp::ZERO,
                AudioLimits::new(256, 8, 32, 512, 64).unwrap(),
            ),
            bank,
            consumer,
        )
        .unwrap();
        let region = PracticeRegion::new(Timestamp::ZERO, end_song(), false).unwrap();
        mixer.install_practice(program, endpoint, region).unwrap();
        let startup = mixer.render(&mut [0.0; CREATION_FRAME as usize]).unwrap();
        assert_eq!(startup.playback_frames, 0);
        assert_eq!(mixer.playback_frame_cursor(), 0);
        mixer.enable_source_playback_history(512).unwrap();
        let pin = mixer.output_frame_basis();
        assert_eq!(pin.start_physical_frame(), CREATION_FRAME);
        let identity = SourcePlaybackIdentity::new(SOURCE_HZ, CREATION_FRAME, 0).unwrap();
        let output = NativeOutputState::new(
            mixer,
            DeviceFormat::new(SOURCE_HZ, 1, SampleEncoding::Float32, None).unwrap(),
            None,
            128,
        )
        .unwrap_or_else(|_| panic!("original complete Source owner"));
        let basis = output.output_frame_basis();
        let descriptor = support::descriptor(basis, frequency);
        let (mut effects, mut native) =
            Effects::for_active_pump(output, EPOCH, Request::normal(frequency), 2, true);
        effects.enable_history_service();
        let mut presentation = new_coordinate_audio_presentation(
            EPOCH,
            descriptor,
            ClockDomainId(1),
            point(3, 0),
            AudioAuthorityConfig {
                history_capacity: 512,
                max_observation_age: Duration::from_nanos(100_000_000),
                input_extrapolation: ExtrapolationPolicy::Forbid,
                max_input_ahead: Duration::ZERO,
            },
        )
        .unwrap();
        // Prime with actual submitted startup silence before the program starts.
        // The first real association at physical7 encloses Started at physical7;
        // a receipt at physical5 would already precede this retained history.
        // Counter zero is unavailable, so a copied Frame-zero seed cannot prove it.
        let seed = effects
            .observe_native_playback(&mut native, identity)
            .unwrap()
            .unwrap();
        presentation.admit_coordinate(seed.coordinate()).unwrap();
        let seeded = seed.source().facts();
        let seed_report = seeded.render.unwrap();
        assert_eq!(seed_report.start_frame, CREATION_FRAME);
        assert_eq!(seed_report.frames as u64, START_FRAME - CREATION_FRAME);
        assert_eq!(seed_report.playback_frames, 0);
        assert!(
            seed_report.paused,
            "the closed gate has zero active extent and reports effective paused output"
        );
        assert!(
            !native.source_state().mixer().pause_requested(),
            "effective startup silence does not acknowledge a manual producer pause"
        );
        assert_eq!(seeded.playback.state(), SourcePlaybackState::Frozen);
        assert_eq!(seeded.playback.playback_start(), 0);
        assert_eq!(seeded.pulled_source_end, START_FRAME);
        assert_eq!(seeded.submitted_source_end, START_FRAME);
        assert_eq!(native.source_state().mixer().applied_start_frame(), None);
        assert_eq!(native.source_state().mixer().playback_frame_cursor(), 0);
        presentation.pin_practice_mixer_basis(pin).unwrap();
        let playback_origin = basis
            .point_at_stream_frame(START_FRAME - CREATION_FRAME)
            .unwrap();
        let logical_start = presentation
            .authority()
            .epoch()
            .logical_output(playback_origin)
            .unwrap();
        let phase = OutputPhaseBasis::new(ClockDomainId(3), 91, 1, 0, SOURCE_HZ).unwrap();
        let mut members = Vec::new();
        let mut configs = Vec::new();
        let mut states = Vec::new();
        let mut transports = Vec::new();
        let mut initial_attempts = Vec::new();
        for &id in ids {
            let launch = SessionLaunch::new(vec![
                "--chart".into(),
                "connected.bms".into(),
                "--speed".into(),
                speed.to_string(),
                "--record-replay".into(),
                format!("player-{id}.bkr"),
            ])
            .unwrap();
            let attempt = prepare_attempt_with_speed(
                &original,
                &policy(),
                &launch,
                PracticeAttemptConfig {
                    start: Timestamp::ZERO,
                    end: Some(end_song()),
                    domain: ClockDomainId(3),
                    chart_seed: 0,
                    capture_limits: Some(limits()),
                },
                speed,
            )
            .unwrap();
            initial_attempts.push(AttemptIdentity {
                player: PlayerId(id),
                launch: launch.clone(),
                header: attempt.capture.as_ref().unwrap().header().clone(),
            });
            transports.push(
                attempt
                    .transport_at_output_phase(phase, logical_start)
                    .unwrap(),
            );
            configs.push(MemberConfig {
                player: PlayerId(id),
                device: cohort.then_some(DeviceId(u64::from(id))),
                bindings: BindingMap::from_bindings([Binding {
                    device: if cohort {
                        DeviceSelector::Exact(DeviceId(u64::from(id)))
                    } else {
                        DeviceSelector::Any
                    },
                    physical: PhysicalControlId::keyboard(7),
                    game_control: GameControlId(0x11),
                }])
                .unwrap(),
                judge: attempt.judge,
                sounds: vec![],
            });
            states.push(GameplayPlayerState {
                player: PlayerId(id),
                capture: attempt.capture,
                competition: None,
                completion: None,
                score: attempt.score,
                gauge: attempt.gauge,
                last_song: Timestamp::ZERO,
            });
            members.push(PracticeMember {
                player: PlayerId(id),
                policy: policy(),
                launch,
                chart_seed: 0,
                capture_limits: Some(limits()),
            });
        }
        let playback = PracticePlayback::new_with_speed(
            controller,
            original.clone(),
            members,
            region,
            Timestamp::from_nanos(200_000_000),
            pin,
            2,
            Some(end_song()),
            setup,
        )
        .unwrap();
        let runtimes = if cohort {
            let mut group = RuntimeGroup::new(
                ClockDomainId(3),
                ClockDomainId(2),
                transports.remove(0),
                producer,
                configs,
                64,
                &[],
            )
            .unwrap();
            group.set_processing_clock(RuntimeProcessingClock::Disabled);
            group.set_audio_scope(CommandScope(1));
            group.set_song_end(end_song()).unwrap();
            Runtimes::Cohort { group, states }
        } else {
            let member = configs.remove(0);
            let state = states.remove(0);
            let mut runtime = SoloRuntime::new(
                ClockDomainId(3),
                ClockDomainId(2),
                transports.remove(0),
                member.bindings,
                member.judge,
                producer,
                vec![],
                64,
            )
            .unwrap();
            runtime.set_processing_clock(RuntimeProcessingClock::Disabled);
            runtime.set_audio_scope(CommandScope(1));
            runtime.set_song_end(end_song()).unwrap();
            Runtimes::Solo {
                runtime,
                gauge: state.gauge,
                capture: state.capture,
            }
        };
        let state = Rc::new(RefCell::new(State {
            attempts: initial_attempts,
            ..Default::default()
        }));
        let mailbox = Rc::new(RefCell::new(Mailbox {
            applied: Some(capability(&Request::normal(frequency))),
            ..Default::default()
        }));
        let owner = Owner::new_with_ticket(effects.clone(), native);
        let changed_request = Request::configured(
            DeviceFormat::new(SOURCE_HZ, 2, SampleEncoding::Float32, Some(3)).unwrap(),
            Some(ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap()),
            16,
            128,
            frequency.map(|_| 10_000_000),
        );
        let device = Device {
            owner,
            ui: GameplayOutputUi::new(Port(mailbox.clone())),
            effects: effects.clone(),
            mailbox,
            state: state.clone(),
            ids: ids.to_vec(),
            sequences: vec![0; ids.len()],
            paused_input_sent: false,
            changed_request,
            pin,
            identity,
            cold_source: None,
            history_race_injected: false,
            query_race_injected: false,
            observation_race_injected: false,
            race,
            polls_at_queue: 0,
        };
        let config = CoordinateGameplayConfig {
            audio: AudioGameplayConfig {
                gameplay: NativeGameplayConfig {
                    origin: point(1, HOST_START),
                    stream_origin: basis.point_at_stream_frame(0).unwrap(),
                    playback_origin,
                    song_origin: Timestamp::ZERO,
                    sample_rate: SOURCE_HZ,
                    end_song: Some(end_song()),
                    advance_lag: Duration::ZERO,
                    seconds: None,
                    pause_supported: true,
                    logical_schedule: true,
                },
                section_start: Timestamp::ZERO,
            },
            source_history: identity,
        };
        let mut merger =
            InputMerger::new_dynamic(ClockDomainId(1), point(1, HOST_START), 4096, 256).unwrap();
        for &id in ids {
            merger.register_source(DeviceId(u64::from(id))).unwrap();
        }
        Self {
            original,
            runtimes,
            device,
            presentation,
            pause: NativePause::new(pin.origin(), ClockDomainId(1), SOURCE_HZ)
                .unwrap()
                .with_start_frame(START_FRAME)
                .unwrap(),
            merger,
            bgm: BgmFeeder::new(
                vec![],
                BgmConfig {
                    output_origin: pin.origin(),
                    sample_rate: SOURCE_HZ,
                    preroll: Duration::ZERO,
                    lookahead: Duration::from_nanos(1),
                    max_pending: 1,
                },
            )
            .unwrap(),
            playback,
            recordings: Recordings {
                files: vec![],
                state: state.clone(),
            },
            host: Host(state),
            control: Control { effects, ticks: 0 },
            config,
        }
    }
    fn run(&mut self) -> NativeGameplayResult<bool> {
        let mut end = None;
        let mut delivery = InputDeliveryTelemetry::new(256, ClockDomainId(1)).unwrap();
        let mut pre = 0;
        match &mut self.runtimes {
            Runtimes::Solo {
                runtime,
                gauge,
                capture,
            } => {
                let mut completion = None;
                let mut competition = Some(NoopSoloCompetition);
                Ok(run_gameplay_audio_coordinate_with_practice_and_ports(
                    &mut self.device,
                    AudioGameplaySession {
                        session: GameplaySession {
                            runtime,
                            gauge,
                            bgm: &mut self.bgm,
                            discipline: &mut self.presentation,
                            pause: &mut self.pause,
                            end: &mut end,
                            completion: &mut completion,
                            capture,
                            competition: &mut competition,
                            delivery: &mut delivery,
                            pre_origin_inputs: &mut pre,
                        },
                        merger: &mut self.merger,
                    },
                    self.config,
                    &mut self.control,
                    &mut self.host,
                    &mut self.playback,
                    &mut self.recordings,
                )?
                .is_some())
            }
            Runtimes::Cohort { group, states } => {
                let session: AudioCohortSession<'_, NoopSoloCompetition, NoopGroupCompetition> =
                    CohortSession {
                        group,
                        network: None,
                        states,
                        merger: &mut self.merger,
                        bgm: &mut self.bgm,
                        discipline: &mut self.presentation,
                        pause: &mut self.pause,
                        end: &mut end,
                        delivery: &mut delivery,
                        pre_origin_inputs: &mut pre,
                    };
                Ok(run_cohort_audio_coordinate_practice_with_results_and_ports(
                    &mut self.device,
                    session,
                    self.config,
                    &mut self.control,
                    &mut self.host,
                    &mut self.playback,
                    &mut self.recordings,
                )?
                .is_some())
            }
        }
    }
    fn verify(&mut self, ids: &[u32]) {
        let state = self.device.state.borrow();
        assert_eq!(state.stage, Stage::Finish);
        assert_eq!(state.practice_replies.len(), 3);
        assert!(
            state.generation >= 4,
            "real autonomous loop and fractional scrub create attempts"
        );
        if self.device.race != Race::Observation {
            assert!(
                state.old_pending_packets > 0,
                "old clock stays live until authorization"
            );
        }
        match self.device.race {
            Race::History => {
                assert!(self.device.history_race_injected);
                assert_eq!(state.history_authorized_deferrals, 1);
            }
            Race::Query => assert!(self.device.query_race_injected),
            Race::Observation => assert!(self.device.observation_race_injected),
        }
        assert!(
            state.mapping_queries > 0,
            "common pump exercised the mutable conditional Source query"
        );
        assert!(!state.history_revisions.is_empty());
        assert!(state
            .history_revisions
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
        let history = self.device.effects.history_requests();
        assert!(history
            .iter()
            .all(|request| request.2 == self.device.identity));
        for epoch in EPOCH..=EPOCH + 2 {
            assert!(
                history.iter().any(|request| request.0 == epoch),
                "real history queue serviced output epoch {epoch}"
            );
        }
        assert!(
            state.suspended_laps > 0,
            "actual waiting candidate suspends old native clock"
        );
        assert_eq!(state.publication.len(), 2);
        assert_eq!(state.publication[0].0, EPOCH);
        assert_eq!(state.publication[0].1, EPOCH + 1);
        assert_eq!(state.publication[1].0, EPOCH + 1);
        assert_eq!(state.publication[1].1, EPOCH + 2);
        assert!(state.publication.iter().all(|entry| entry.2 == 1
            && entry.3 == self.device.pin
            && entry.4 == self.device.identity));
        assert!(state
            .paused_service
            .iter()
            .all(|(_, pending, phase, _)| *pending == 0 && *phase == PausePhase::Paused));
        assert!(state
            .paused_service
            .iter()
            .any(|(suspended, _, _, _)| *suspended));
        assert_eq!(state.paused_sequences.len(), ids.len() * 2);
        assert!(state
            .pause_states
            .windows(2)
            .any(|pair| pair == [PauseState::Resuming, PauseState::Running]));
        assert_eq!(
            self.presentation.practice_mixer_basis(),
            Some(self.device.pin)
        );
        assert_eq!(self.presentation.authority().epoch().id, EPOCH + 2);
        assert_eq!(
            self.pause.coordinate_source_identity(),
            Some(self.device.identity)
        );
        assert!(self.playback.terminal_ready());
        assert_eq!(self.playback.section_end(), Some(end_song()));
        assert_eq!(state.ends.last(), Some(&end_song()));
        let mailbox = self.device.mailbox.borrow();
        assert_eq!(
            mailbox
                .replies
                .iter()
                .map(|reply| reply.id)
                .collect::<Vec<_>>(),
            [101, 102, 103]
        );
        assert_eq!(
            mailbox
                .reply_attempts
                .iter()
                .filter(|reply| reply.id == 101)
                .count(),
            2,
            "WouldBlock retries only delivery"
        );
        assert_eq!(
            mailbox.applied,
            Some(capability(&self.device.changed_request))
        );
        let native = self.device.owner.current().unwrap();
        assert_eq!(native.format(), self.device.changed_request.format());
        assert_eq!(
            self.presentation.authority().coordinate_basis(),
            Some(support::descriptor(
                native.basis,
                self.device.changed_request.frequency
            )),
            "publication keeps the original coordinate class and actual changed Counter frequency"
        );
        assert_eq!(self.device.pin.start_physical_frame(), CREATION_FRAME);
        assert_eq!(self.device.identity.initial_physical(), CREATION_FRAME);
        assert_eq!(self.device.identity.initial_playback(), 0);
        assert_eq!(
            native
                .source_state()
                .mixer()
                .source_playback_history()
                .unwrap()
                .source_rate(),
            SOURCE_HZ
        );
        let pcm = self.device.effects.pump_pcm();
        assert!(
            pcm.iter()
                .any(|(_, samples)| samples.iter().any(|sample| *sample != 0.0)),
            "real nonzero BGM submitted"
        );
        assert!(
            pcm.iter().any(|(epoch, samples)| *epoch == EPOCH + 2
                && samples
                    .chunks_exact(2)
                    .any(|pair| pair[0] != 0.0 && pair[1] == pair[0] * 0.5)),
            "actual changed matrix reaches submitted PCM"
        );
        let submitted_active_frames: usize = pcm
            .iter()
            .map(|(epoch, samples)| {
                let channels = if *epoch == EPOCH + 2 { 2 } else { 1 };
                samples
                    .chunks_exact(channels)
                    .filter(|frame| {
                        assert!(
                            frame[0] == 0.0 || frame[0] == 0.25,
                            "original program PCM is preserved"
                        );
                        if channels == 2 {
                            assert_eq!(
                                frame[1],
                                frame[0] * 0.5,
                                "every changed-matrix PCM frame is preserved"
                            );
                        }
                        frame[0] != 0.0
                    })
                    .count()
            })
            .sum();
        let traced_frames: usize = pcm
            .iter()
            .map(|(epoch, samples)| {
                let channels = if *epoch == EPOCH + 2 { 2 } else { 1 };
                assert_eq!(samples.len() % channels, 0);
                samples.len() / channels
            })
            .sum();
        let source = native.source_state();
        assert_eq!(
            state.direct_held_admitted, 2,
            "two measured held-prefix admissions"
        );
        assert_eq!(
            traced_frames as u64 + state.direct_held_admitted + source.pending_frames() as u64,
            source.mixer().frame_cursor() - CREATION_FRAME,
            "all generated PCM frames are either positively admitted exactly once or retained as the exact pending suffix"
        );
        assert!(source.mixer().practice_finished());
        assert_eq!(source.pending_frames(), 0);
        assert!(source.pending_samples().is_empty());
        let final_report = native.last.unwrap();
        assert!(!final_report.paused);
        assert_eq!(final_report.frames, 2);
        assert_eq!(final_report.playback_frames, final_report.frames);
        assert_eq!(source.admitted_frames(), final_report.frames);
        assert_eq!(final_report.active_voices, 0);
        assert_eq!(
            final_report.playback_start_frame + final_report.playback_frames as u64,
            source.mixer().playback_frame_cursor()
        );
        let (final_epoch, final_samples) = pcm.last().unwrap();
        assert_eq!(*final_epoch, EPOCH + 2);
        assert_eq!(final_samples.len(), final_report.frames * 2);
        assert_eq!(final_samples.as_slice(), &[0.25, 0.125, 0.0, 0.0]);
        // Ended clears the original voice before mixing its active Source frame.
        // The scheduling cursor still counts that actual submitted silent frame.
        let terminal_silent_frames = final_samples
            .chunks_exact(2)
            .rev()
            .take_while(|frame| frame.iter().all(|sample| *sample == 0.0))
            .count();
        assert_eq!(terminal_silent_frames, final_report.playback_frames - 1);
        assert_eq!(
            submitted_active_frames as u64 + terminal_silent_frames as u64,
            source.mixer().playback_frame_cursor(),
            "original audible PCM plus the proved active Ended silence conserves the complete playback cursor"
        );
        let paused = state.paused_sequences.clone();
        let acquired = state.acquired.clone();
        drop(mailbox);
        drop(state);
        // Decode the actual encoded archived and final logs. No capture is
        // reconstructed from test-side reports or copied into a synthetic file.
        let mut files = std::mem::take(&mut self.recordings.files);
        match &mut self.runtimes {
            Runtimes::Solo {
                runtime, capture, ..
            } => {
                let hash = runtime.judge().stable_hash().unwrap();
                if let Some(capture) = capture.take() {
                    let member = self
                        .playback
                        .members()
                        .iter()
                        .find(|member| member.player == PlayerId(1))
                        .unwrap();
                    let path = member
                        .launch
                        .args()
                        .chunks_exact(2)
                        .find(|pair| pair[0] == "--record-replay")
                        .unwrap();
                    files.push(
                        self.recordings
                            .encoded_attempt(PlayerId(1), Path::new(&path[1]), capture, Some(hash))
                            .unwrap(),
                    );
                }
            }
            Runtimes::Cohort { group, states } => {
                assert!(!group.poisoned());
                for state in states {
                    let hash = group
                        .member_judge(state.player)
                        .unwrap()
                        .stable_hash()
                        .unwrap();
                    if let Some(capture) = state.capture.take() {
                        let member = self
                            .playback
                            .members()
                            .iter()
                            .find(|member| member.player == state.player)
                            .unwrap();
                        let path = member
                            .launch
                            .args()
                            .chunks_exact(2)
                            .find(|pair| pair[0] == "--record-replay")
                            .unwrap();
                        files.push(
                            self.recordings
                                .encoded_attempt(
                                    state.player,
                                    Path::new(&path[1]),
                                    capture,
                                    Some(hash),
                                )
                                .unwrap(),
                        );
                    }
                }
            }
        }
        assert!(files.len() >= ids.len() * 4);
        assert_eq!(
            files.iter().filter(|file| file.live_hash.is_some()).count(),
            ids.len()
        );
        for member in self.playback.members() {
            let archived: Vec<_> = files
                .iter()
                .filter(|file| file.player == member.player && file.live_hash.is_none())
                .collect();
            assert_eq!(archived.len(), member.launch.attempt() as usize);
            assert!(archived
                .iter()
                .enumerate()
                .all(|(ordinal, file)| file.identity.launch.attempt() as usize == ordinal));
        }
        let mut observed_input = 0;
        for recorded in files {
            let player = recorded.player;
            let file = decode_replay(&recorded.bytes, limits()).unwrap();
            assert_eq!(file.header, recorded.identity.header);
            assert_eq!(
                file.records, recorded.accepted,
                "decoded archived/final capture equals the exact accepted member prefix"
            );
            let path = recorded
                .identity
                .launch
                .args()
                .chunks_exact(2)
                .find(|pair| pair[0] == "--record-replay")
                .unwrap();
            assert_eq!(recorded.path, Path::new(&path[1]));
            assert!(file
                .records
                .windows(2)
                .all(|pair| pair[0].ordinal < pair[1].ordinal
                    && pair[0].song_time <= pair[1].song_time));
            for record in &file.records {
                if let ReplayOperation::Input(input) = &record.operation {
                    observed_input += 1;
                    let meta = input.physical.meta();
                    assert_eq!(meta.source, DeviceId(u64::from(player.0)));
                    assert!(
                        !paused.contains(&(meta.source, meta.sequence)),
                        "paused Down/Up levels do not become gameplay input"
                    );
                    let original = acquired
                        .iter()
                        .find(|event| {
                            event.meta().source == meta.source
                                && event.meta().sequence == meta.sequence
                        })
                        .expect("only acquired original input is recorded");
                    assert_eq!(
                        meta.original_clock_point,
                        Some(ClockPoint {
                            domain: original.meta().clock_domain,
                            timestamp: original.meta().timestamp
                        })
                    );
                }
            }
            let replay =
                crate::replay_playback::reconstruct_section(&self.original, file, limits())
                    .unwrap();
            assert_eq!(replay.results(), recorded.events.as_slice(), "decoded replay emits the actual member's complete retired/final judge result prefix");
            if let Some(hash) = recorded.live_hash {
                assert_eq!(replay.engine().stable_hash().unwrap(), hash);
            }
        }
        assert!(observed_input > 0);
    }
}

#[test]
fn native_retained_source_output_replacement_frame_solo_history_authorization_preserves_real_practice(
) {
    let mut f = Fixture::new(&[1], false, None, Race::History);
    assert!(f.run().unwrap());
    f.verify(&[1]);
}
#[test]
fn native_retained_source_output_replacement_counter_solo_observation_authorization_preserves_real_practice(
) {
    let mut f = Fixture::new(&[1], false, Some(support::OLD_FREQUENCY), Race::Observation);
    assert!(f.run().unwrap());
    f.verify(&[1]);
}
#[test]
fn native_retained_source_output_replacement_frame_cohort_query_authorization_preserves_distinct_captures(
) {
    let mut f = Fixture::new(&[7, 29], true, None, Race::Query);
    assert!(f.run().unwrap());
    f.verify(&[7, 29]);
}
#[test]
fn native_retained_source_output_replacement_counter_cohort_history_authorization_preserves_distinct_captures(
) {
    let mut f = Fixture::new(&[7, 29], true, Some(support::OLD_FREQUENCY), Race::History);
    assert!(f.run().unwrap());
    f.verify(&[7, 29]);
}
