//! Connected native policy tests. Scripted submitted output is not hardware timing proof.
use crate as runtime;
#[path = "gameplay/output/application/coordinate_target_replacement_test_support.rs"]
mod support;
use crate::{
    audio_authority::{AudioAuthorityConfig, AudioAuthorityCoordinateBasis},
    bgm::{BgmConfig, BgmFeeder},
    gameplay::output::{
        application::{owner::GameplayOutputOwner, requests::GameplayOutputUi},
        domain::{
            control::{OutputCapability, OutputReply, OutputRequest},
            remix::matrix_text,
        },
        ports::{
            OriginalTargetNativeOutputBackend, OutputUiPort, SourcePlaybackIdentity,
            TargetRetirementStatus,
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
use beatkernel_platform::audio::{ConvertedNativeOutputState, DeviceFormat, SampleEncoding};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration as WallDuration,
};
use support::{point, snapshot, Effects, Fault, Request, Ticket, HOST_START};

#[derive(Debug, PartialEq, Eq)]
struct ColdSource {
    generation: Option<u64>,
    frozen: u64,
    pulled: u64,
    position: SourcePosition,
    history: support::SourceHistorySnapshot,
}
fn cold_source(output: &support::Output) -> ColdSource {
    let source = output.source_state();
    ColdSource {
        generation: source.mixer().practice_generation(),
        frozen: source.mixer().playback_frame_cursor(),
        pulled: source.mixer().frame_cursor(),
        position: source.converter_owner().source_position(),
        history: support::source_history(source),
    }
}
#[derive(Clone)]
struct AttemptIdentity {
    player: PlayerId,
    launch: SessionLaunch,
    header: ReplayHeader,
}

type Owner = GameplayOutputOwner<Effects, ConvertedNativeOutputState, TargetFrameBasis, Ticket>;
const SOURCE_HZ: u32 = 1000;
const TARGET_HZ: u32 = 1500;
const EPOCH: u64 = 7;
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
        host: SettingsHost::Linux,
        current_args: vec![
            "--alsa".into(),
            "connected-memory".into(),
            "--rate".into(),
            request.format().sample_rate().to_string(),
            "--output-matrix".into(),
            matrix_text(request.matrix()).unwrap(),
            "--encoding".into(),
            "float32".into(),
            "--period-frames".into(),
            request.period().to_string(),
            "--buffer-frames".into(),
            request.buffer().to_string(),
        ],
    };
    cap.validate().unwrap();
    cap
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
}
impl Device {
    fn queue(&mut self, id: u64, request: Request) {
        assert!(self.cold_source.is_none());
        self.cold_source = Some(cold_source(self.owner.current().unwrap()));
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
        let packet = self
            .owner
            .observe_conditional_target_native_control(presentation, identity)?;
        if was_pending && packet.is_some() {
            self.state.borrow_mut().old_pending_packets += 1;
        }
        if self.owner.output_clock_suspended() {
            self.state.borrow_mut().suspended_laps += 1;
        }
        Ok(packet.map(CoordinatePlaybackControlFacts::Target))
    }
    fn playback_mapping_at(
        &self,
        presentation: &NativeAudioPresentation,
        query: &InterpolatedTargetTime,
        identity: SourcePlaybackIdentity,
    ) -> NativeGameplayResult<Option<NativePlaybackMapping>> {
        Ok(self
            .owner
            .target_playback_at(presentation, query, identity)?
            .as_ref()
            .map(NativePlaybackMapping::from_target))
    }
    fn output_clock_suspended(&self) -> bool {
        self.owner.output_clock_suspended()
    }
    fn output_replacement_pending(&self) -> bool {
        self.owner.has_work() || self.ui.pending()
    }
    fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
        self.effects.set_held(
            self.owner
                .current_mut()
                .ok_or("held command without published output")?,
            held,
        )?;
        Ok(())
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
                self.owner.current_mut().unwrap().pump_partial_held(4, 1);
                self.effects.pump_admit_limit(1);
                self.effects.conditional_statuses([
                    TargetRetirementStatus::Pending,
                    TargetRetirementStatus::Pending,
                    TargetRetirementStatus::Pending,
                    TargetRetirementStatus::Pending,
                    TargetRetirementStatus::Pending,
                    TargetRetirementStatus::Pending,
                    TargetRetirementStatus::Authorized,
                ]);
                self.effects.observation_faults([
                    Fault::Missing,
                    Fault::Missing,
                    Fault::None,
                    Fault::Missing,
                    Fault::Missing,
                ]);
                self.mailbox.borrow_mut().block_next = true;
                self.queue(101, Request::normal(TARGET_HZ));
                let mut state = self.state.borrow_mut();
                state.stage = Stage::ReplacingFirst;
                state.desired_pause = false;
            }
            Stage::SecondPaused => {
                self.owner.current_mut().unwrap().pump_partial_held(4, 1);
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
                    "the live refusal has a genuine incompatible retained suffix"
                );
                self.effects.pump_admit_limit(1);
                self.effects
                    .conditional_statuses([TargetRetirementStatus::Refused]);
                self.queue(102, self.changed_request.clone());
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
                    .conditional_statuses([TargetRetirementStatus::Authorized]);
                self.queue(103, self.changed_request.clone());
                self.state.borrow_mut().stage = Stage::ReplacingChanged;
            }
            _ => {}
        }
        let before_epoch = context.presentation.authority().epoch().id;
        let generation = self.state.borrow().generation;
        let mapped = self.mailbox.clone();
        let published = self.ui.service_coordinate_target_audio(
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
            assert_eq!(
                cold_source(output),
                self.cold_source
                    .take()
                    .expect("every actual publication has its cold original owner")
            );
            assert!(
                self.state.borrow().archived_attempts.is_empty(),
                "output replacement itself never archives an attempt"
            );
            assert!(output.pump_held());
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
    fn new(ids: &[u32], cohort: bool) -> Self {
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
        let (producer, consumer) = command_queue(256).unwrap();
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
        let pin = mixer.output_frame_basis();
        let identity =
            SourcePlaybackIdentity::new(SOURCE_HZ, pin.start_physical_frame(), 0).unwrap();
        let mut output = ConvertedNativeOutputState::new(
            mixer,
            DeviceFormat::new(TARGET_HZ, 1, SampleEncoding::Float32, None).unwrap(),
            ChannelMatrix::default_mix(1, 1).unwrap(),
            ResampleQuality::Linear,
            128,
        )
        .unwrap_or_else(|_| panic!("original converter"));
        output.enable_source_playback_history(512).unwrap();
        output.enable_phase_history(512).unwrap();
        let basis = output.target_frame_basis();
        let (effects, native) =
            Effects::for_active_pump(output, EPOCH, Request::normal(TARGET_HZ), 2, true);
        let mut presentation = new_coordinate_audio_presentation(
            EPOCH,
            AudioAuthorityCoordinateBasis::TargetFrame(basis),
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
        presentation
            .admit_target_coordinate(snapshot(EPOCH, basis, 0, HOST_START))
            .unwrap();
        presentation.pin_practice_mixer_basis(pin).unwrap();
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
                    .transport_at_output_phase(phase, point(3, 0))
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
            applied: Some(capability(&Request::normal(TARGET_HZ))),
            ..Default::default()
        }));
        let owner = Owner::new_with_ticket(effects.clone(), native);
        let changed_request = Request::configured(
            DeviceFormat::new(2250, 2, SampleEncoding::Float32, Some(3)).unwrap(),
            ChannelMatrix::new(1, 2, &[1.0, 0.5]).unwrap(),
            16,
            128,
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
        };
        let config = CoordinateGameplayConfig {
            audio: AudioGameplayConfig {
                gameplay: NativeGameplayConfig {
                    origin: point(1, HOST_START),
                    stream_origin: basis.origin(),
                    playback_origin: basis.origin(),
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
                .with_target_basis(EPOCH, basis)
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
        assert!(
            state.old_pending_packets > 0,
            "old original clock stays live until authorization"
        );
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
            native
                .source_state()
                .converter_owner()
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
fn native_retained_output_replacement_solo_drains_input_preserves_capture_and_completes_real_practice(
) {
    let mut fixture = Fixture::new(&[1], false);
    assert!(fixture.run().unwrap());
    fixture.verify(&[1]);
}

#[test]
fn native_retained_output_replacement_two_member_cohort_uses_one_owner_distinct_captures_and_real_practice(
) {
    let mut fixture = Fixture::new(&[7, 29], true);
    assert!(fixture.run().unwrap());
    fixture.verify(&[7, 29]);
}

fn transition_setup() -> (PracticePlayback, Mixer, OutputFrameBasis) {
    transition_setup_with_end(end_song())
}
fn transition_setup_with_end(end: Timestamp) -> (PracticePlayback, Mixer, OutputFrameBasis) {
    let original =
        beatkernel_bms::parse("#BPM 120\n#WAV01 key.wav\n#00011:01\n", Default::default()).unwrap();
    let format = AudioFormat::new(SOURCE_HZ, 1).unwrap();
    let pcm_limits = PcmLimits::new(4096, 8192, 2).unwrap();
    let mut bank = SampleBank::new(format, pcm_limits).unwrap();
    bank.insert(
        SampleId(1),
        PcmSample::new(format, vec![0.25; 128], pcm_limits).unwrap(),
    )
    .unwrap();
    let program = PreparedPracticeProgram::new(
        &bank,
        vec![PracticeCue {
            voice: VoiceId(1),
            sample: SampleId(1),
            at: Timestamp::ZERO,
            gain: 1.0,
        }],
        PracticeLimits::new(4, 2, 64, 8, 64).unwrap(),
    )
    .unwrap();
    let (controller, endpoint) = practice_queue(&program).unwrap();
    let (_, consumer) = command_queue(64).unwrap();
    let mut mixer = Mixer::new(
        MixerConfig::new(
            format,
            ClockDomainId(2),
            Timestamp::ZERO,
            AudioLimits::new(64, 4, 16, 256, 16).unwrap(),
        ),
        bank,
        consumer,
    )
    .unwrap();
    let region = PracticeRegion::new(Timestamp::ZERO, end, false).unwrap();
    mixer.install_practice(program, endpoint, region).unwrap();
    let pin = mixer.output_frame_basis();
    let playback = PracticePlayback::new(
        controller,
        original,
        vec![PracticeMember {
            player: PlayerId(1),
            policy: policy(),
            launch: SessionLaunch::new(vec!["--chart".into(), "transition.bms".into()]).unwrap(),
            chart_seed: 0,
            capture_limits: None,
        }],
        region,
        Timestamp::from_nanos(200_000_000),
        pin,
        2,
    )
    .unwrap();
    (playback, mixer, pin)
}

#[test]
fn native_retained_output_replacement_transition_fence_covers_raw_projected_held_and_pre_receipt_request(
) {
    let (mut raw, mixer, pin) = transition_setup_with_end(Timestamp::from_nanos(4_000_000));
    assert!(!raw.transition_pending());
    let mut converted = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(TARGET_HZ, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        16,
    )
    .unwrap_or_else(|_| panic!("raw lookahead converter"));
    let basis = converted.target_frame_basis();
    converted.render_pending(6).unwrap();
    converted.admit(6).unwrap();
    assert!(converted.mixer().frame_cursor() > 4);
    // Like the existing retention fixture, the generated samples are 0..5.
    // Source4 needs Target6, so source lookahead has not projected Ended yet.
    let mut presentation = new_coordinate_audio_presentation(
        EPOCH,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
        ClockDomainId(1),
        point(3, 0),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    presentation
        .admit_target_coordinate(snapshot(EPOCH, basis, 0, HOST_START))
        .unwrap();
    presentation
        .admit_target_coordinate(snapshot(EPOCH, basis, 6, HOST_START + 6_000_000))
        .unwrap();
    presentation.pin_practice_mixer_basis(pin).unwrap();
    let now = point(1, HOST_START + 6_000_000);
    let merger =
        InputMerger::new_dynamic(ClockDomainId(1), point(1, HOST_START), 4096, 16).unwrap();
    presentation
        .authority_mut()
        .record_acquired_prefix(now)
        .unwrap();
    let boundary = raw.qualify(&presentation, now).unwrap().unwrap();
    let mut host = Host(Rc::new(RefCell::new(State::default())));
    let mut started = raw.prepare_boundary(&boundary, &mut host).unwrap();
    assert_eq!(started.receipt.kind, PracticeBoundaryKind::Started);
    presentation
        .commit_practice_boundary(boundary, now, &merger)
        .unwrap();
    raw.apply_boundary(&mut started).unwrap();
    assert!(raw.history_retention_pins().is_none());
    let (source, target) = raw.queued_history_retention_pins();
    assert_eq!(source, Some(4));
    assert!(target.is_none());
    assert!(
        raw.transition_pending(),
        "unprojected Source receipt already prevents admission"
    );
    assert!(raw.peek_receipt().unwrap().is_none());
    assert!(
        raw.transition_pending(),
        "polling projected receipts cannot hide an original raw pin"
    );

    let (mut playback, mixer, pin) = transition_setup();
    let mut output = ConvertedNativeOutputState::new(
        mixer,
        DeviceFormat::new(TARGET_HZ, 1, SampleEncoding::Float32, None).unwrap(),
        ChannelMatrix::default_mix(1, 1).unwrap(),
        ResampleQuality::Linear,
        32,
    )
    .unwrap_or_else(|_| panic!("converter"));
    output.enable_source_playback_history(64).unwrap();
    output.enable_phase_history(64).unwrap();
    let basis = output.target_frame_basis();
    assert!(!playback.transition_pending());
    output.render_pending(4).unwrap();
    output.admit(4).unwrap();
    let (source, target) = playback.queued_history_retention_pins();
    assert!(source.is_some() && target.is_some());
    assert!(playback.transition_pending());
    let receipt = playback.peek_receipt().unwrap().unwrap();
    assert_eq!(receipt.kind, PracticeBoundaryKind::Started);
    assert!(playback.history_retention_pins().is_some());
    assert!(
        playback.transition_pending(),
        "moving a queued receipt into the held slot preserves the fence"
    );
    let mut presentation = new_coordinate_audio_presentation(
        EPOCH,
        AudioAuthorityCoordinateBasis::TargetFrame(basis),
        ClockDomainId(1),
        point(3, 0),
        AudioAuthorityConfig::default(),
    )
    .unwrap();
    presentation
        .admit_target_coordinate(snapshot(EPOCH, basis, 0, HOST_START))
        .unwrap();
    presentation
        .admit_target_coordinate(snapshot(EPOCH, basis, 4, HOST_START + 4_000_000))
        .unwrap();
    presentation.pin_practice_mixer_basis(pin).unwrap();
    let now = point(1, HOST_START + 4_000_000);
    let merger =
        InputMerger::new_dynamic(ClockDomainId(1), point(1, HOST_START), 4096, 16).unwrap();
    presentation
        .authority_mut()
        .record_acquired_prefix(now)
        .unwrap();
    let boundary = playback.qualify(&presentation, now).unwrap().unwrap();
    let state = Rc::new(RefCell::new(State::default()));
    let mut host = Host(state.clone());
    let mut prepared = playback.prepare_boundary(&boundary, &mut host).unwrap();
    presentation
        .commit_practice_boundary(boundary, now, &merger)
        .unwrap();
    let applied = playback.apply_boundary(&mut prepared).unwrap();
    playback.publish_boundary(&applied, &mut host).unwrap();
    assert!(
        !playback.transition_pending(),
        "actual Started application clears all original receipt owners"
    );
    {
        let mut state = state.borrow_mut();
        state.stage = Stage::Loop;
        state.generation = playback.generation();
    }
    playback
        .service_request(&mut host, output.mixer().playback_frame_cursor())
        .unwrap();
    assert_eq!(state.borrow().practice_sent.len(), 1);
    let (source, target) = playback.queued_history_retention_pins();
    assert!(source.is_none() && target.is_none());
    assert!(playback.peek_receipt().unwrap().is_none());
    assert!(
        playback.transition_pending(),
        "admitted request is fenced before the audio owner makes its receipt"
    );
    output.render_pending(4).unwrap();
    output.admit(4).unwrap();
    assert!(playback.transition_pending());
    assert_eq!(
        playback.peek_receipt().unwrap().unwrap().kind,
        PracticeBoundaryKind::Requested
    );
    assert!(playback.transition_pending());
}
