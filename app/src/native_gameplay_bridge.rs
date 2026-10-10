//! Compatibility composition: legacy player publication and native pump effects.

use crate::{
    competition_live::LiveCompetition,
    gameplay_competition::{GroupCompetitionPort, SoloCompetitionPort},
    gameplay_presentation::{GameplayDevice, GameplayPresentationPort},
    live_pause::LivePauseObservation,
    local_players::PlayerId,
    local_runtime::PlayerReport,
    multiplayer_group::MemberProgress,
    native_cohort::{
        finite_cohort_done_for_states, member_progress_for_states, run_cohort_with_ports,
        run_cohort_with_results_and_ports, CohortSession, GameplayPlayerState,
    },
    native_end::{EndBoundary, NativeEnd},
    native_gameplay::{
        run_gameplay_with_ports, run_gameplay_with_result_and_ports, GameplaySession, InputBatch,
        NativeGameplayConfig, NativeGameplayResult,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_group_competition::NativeGroupCompetition,
    native_pump_control::NativePumpControl,
    native_pump_system::SystemControl,
    play_result::CompletedPlayResult,
    player,
};
use beatkernel::{
    audio::RenderReport,
    input::PhysicalInputEvent,
    runtime::RuntimeReport,
    time::{
        presentation::{DisciplineConfig, DisciplineUpdate},
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp,
    },
    transport::Transport,
};

use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
use std::collections::VecDeque;

pub trait NativeGameplayDevice {
    fn output_clock_suspended(&self) -> bool {
        false
    }
    fn output_replacement_pending(&self) -> bool {
        false
    }
    /// Converted owners switch native held output at acknowledged target boundaries.
    fn set_audio_held(&mut self, _: bool) -> NativeGameplayResult<()> {
        Ok(())
    }
    /// Compatibility hook for committed paused replacement; existing owners opt out.
    fn publish_paused_output(
        &mut self,
        _: crate::gameplay_presentation::GameplayOutputContext<'_, PresentationDiscipline>,
    ) -> NativeGameplayResult<bool> {
        Ok(false)
    }
    fn observe_audio(
        &mut self,
        _: &mut crate::native_audio_presentation::NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        Err("native device does not support original audio presentation".into())
    }
    fn audio_pause_observation(
        &mut self,
        _: &crate::native_audio_presentation::NativeAudioPresentation,
        _: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        Err("native device does not support original audio pause evidence".into())
    }
    fn observe_audio_end(
        &mut self,
        _: &mut NativeEnd,
        _: &crate::native_audio_presentation::NativeAudioPresentation,
        _: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        Err("native device does not support original audio end evidence".into())
    }
    fn publish_paused_audio_output(
        &mut self,
        _: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
        _: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        Err("native device does not support held audio output publication".into())
    }
    fn observe(&mut self, discipline: &mut PresentationDiscipline) -> NativeGameplayResult<()>;
    /// Native interval owners override this with original coherent evidence;
    /// correction-only midpoint pairs cannot establish their pause boundary.
    fn pause_observation(
        &mut self,
        reference: ClockPair,
    ) -> NativeGameplayResult<LivePauseObservation> {
        Ok(LivePauseObservation::Point(reference))
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>>;
    fn host_now(&self) -> NativeGameplayResult<ClockPoint>;
    /// Append original input before returning its optional native completed cut.
    /// Receipt time and transfer queue emptiness do not establish completion.
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch>;
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &PresentationDiscipline,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>>;
    /// Reseed using the original native observation source, never a fabricated snapshot.
    /// Success must leave an accepted latest pair in the staged observer.
    fn seed_resume(
        &mut self,
        discipline: &mut PresentationDiscipline,
        reference: ClockPair,
    ) -> NativeGameplayResult<()>;
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint>;
}
impl GameplayPresentationPort for PresentationDiscipline {
    fn new_with_playback_origin(
        config: DisciplineConfig,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        host_domain: ClockDomainId,
        applied_song_origin: Timestamp,
    ) -> NativeGameplayResult<Self> {
        Ok(Self::new_with_playback_origin(
            config,
            output_origin,
            playback_origin,
            host_domain,
            applied_song_origin,
        )?)
    }
    fn config(&self) -> DisciplineConfig {
        Self::config(self)
    }
    fn epoch(&self) -> Option<u64> {
        Some(Self::epoch(self))
    }
    fn rebind_output(
        &mut self,
        epoch: u64,
        output_origin: ClockPoint,
        playback_origin: ClockPoint,
        song_origin: Timestamp,
    ) -> NativeGameplayResult<()> {
        Ok(Self::rebind_output(
            self,
            epoch,
            output_origin,
            playback_origin,
            song_origin,
        )?)
    }
    fn latest_pair(&self) -> Option<ClockPair> {
        Self::latest_pair(self)
    }
    fn quality(&self) -> ClockMappingQuality {
        Self::quality(self)
    }
    fn validate_host(&self, point: ClockPoint) -> NativeGameplayResult<()> {
        Ok(Self::validate_host(self, point)?)
    }
    fn update(
        &mut self,
        now: ClockPoint,
        transport: &mut Transport,
    ) -> NativeGameplayResult<DisciplineUpdate> {
        Ok(Self::update(self, now, transport)?)
    }
}

impl<D: NativeGameplayDevice> GameplayDevice for D {
    type Presentation = PresentationDiscipline;
    fn output_clock_suspended(&self) -> bool {
        NativeGameplayDevice::output_clock_suspended(self)
    }
    fn output_replacement_pending(&self) -> bool {
        NativeGameplayDevice::output_replacement_pending(self)
    }
    fn publish_paused_output(
        &mut self,
        context: crate::gameplay_presentation::GameplayOutputContext<'_, Self::Presentation>,
    ) -> NativeGameplayResult<bool> {
        NativeGameplayDevice::publish_paused_output(self, context)
    }
    fn observe_audio(
        &mut self,
        presentation: &mut crate::native_audio_presentation::NativeAudioPresentation,
    ) -> NativeGameplayResult<()> {
        NativeGameplayDevice::observe_audio(self, presentation)
    }
    fn audio_pause_observation(
        &mut self,
        presentation: &crate::native_audio_presentation::NativeAudioPresentation,
        now: ClockPoint,
    ) -> NativeGameplayResult<LivePauseObservation> {
        NativeGameplayDevice::audio_pause_observation(self, presentation, now)
    }
    fn set_audio_held(&mut self, held: bool) -> NativeGameplayResult<()> {
        NativeGameplayDevice::set_audio_held(self, held)
    }
    fn observe_audio_end(
        &mut self,
        end: &mut NativeEnd,
        presentation: &crate::native_audio_presentation::NativeAudioPresentation,
        rendered: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        NativeGameplayDevice::observe_audio_end(self, end, presentation, rendered)
    }
    fn publish_paused_audio_output(
        &mut self,
        context: crate::gameplay_presentation::GameplayAudioOutputContext<'_>,
        now: ClockPoint,
    ) -> NativeGameplayResult<bool> {
        NativeGameplayDevice::publish_paused_audio_output(self, context, now)
    }
    fn observe(&mut self, discipline: &mut Self::Presentation) -> NativeGameplayResult<()> {
        NativeGameplayDevice::observe(self, discipline)
    }
    fn pause_observation(
        &mut self,
        reference: ClockPair,
    ) -> NativeGameplayResult<LivePauseObservation> {
        NativeGameplayDevice::pause_observation(self, reference)
    }
    fn render_report(&mut self) -> NativeGameplayResult<Option<RenderReport>> {
        NativeGameplayDevice::render_report(self)
    }
    fn host_now(&self) -> NativeGameplayResult<ClockPoint> {
        NativeGameplayDevice::host_now(self)
    }
    fn acquire(
        &mut self,
        events: &mut VecDeque<PhysicalInputEvent>,
    ) -> NativeGameplayResult<InputBatch> {
        NativeGameplayDevice::acquire(self, events)
    }
    fn observe_end(
        &mut self,
        end: &mut NativeEnd,
        discipline: &Self::Presentation,
        report: Option<RenderReport>,
    ) -> NativeGameplayResult<Option<EndBoundary>> {
        NativeGameplayDevice::observe_end(self, end, discipline, report)
    }
    fn seed_resume(
        &mut self,
        discipline: &mut Self::Presentation,
        reference: ClockPair,
    ) -> NativeGameplayResult<()> {
        NativeGameplayDevice::seed_resume(self, discipline, reference)
    }
    fn fallback_schedule(&mut self, rate: u32) -> NativeGameplayResult<ClockPoint> {
        NativeGameplayDevice::fallback_schedule(self, rate)
    }
}

/// Compatibility specialization selected by native application composition.
pub type NativeGameplaySession<'a> = GameplaySession<'a, LiveCompetition, PresentationDiscipline>;
/// Compatibility cohort specialization with the actual native competition owners.
pub type NativeCohortSession<'a> =
    CohortSession<'a, LiveCompetition, NativeGroupCompetition, PresentationDiscipline>;
/// Compatibility per-member state for native comparison owners.
pub type PlayerState = GameplayPlayerState<LiveCompetition>;

impl SoloCompetitionPort for LiveCompetition {
    fn prepare_practice(
        &self,
        attempt: &crate::practice_session::PreparedPracticeAttempt,
        policy: &crate::play_policy::ResolvedPlayPolicy,
    ) -> NativeGameplayResult<Self> {
        LiveCompetition::prepare_practice(self, attempt, policy)
    }
    fn expected_policy_header(&self) -> Option<&beatkernel::replay::ReplayHeader> {
        Some(self.native_policy_header())
    }
    fn observe(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        LiveCompetition::observe(self, report)
    }
    fn mark_native_completed(&mut self) {
        LiveCompetition::mark_native_completed(self);
    }
}

impl GroupCompetitionPort for NativeGroupCompetition {
    fn expected_policy_header(
        &self,
        player: crate::local_players::PlayerId,
    ) -> Option<&beatkernel::replay::ReplayHeader> {
        self.native_policy_header(player)
    }
    fn observe(&mut self, members: &[MemberProgress]) -> NativeGameplayResult<()> {
        NativeGroupCompetition::observe(self, members)
    }
    fn mark_native_completed(&mut self) {
        NativeGroupCompetition::mark_native_completed(self);
    }
}

/// Native compatibility wrapper, including type inference for an empty roster.
pub fn member_progress(states: &[PlayerState]) -> NativeGameplayResult<Vec<MemberProgress>> {
    member_progress_for_states(states)
}

/// Native compatibility wrapper over the unchanged finite frontier policy.
pub fn finite_cohort_done(
    end: Option<i64>,
    presented: Option<ClockPoint>,
    committed: Option<ClockPoint>,
    states: &[PlayerState],
    backlog: bool,
    resuming: bool,
) -> bool {
    finite_cohort_done_for_states(end, presented, committed, states, backlog, resuming)
}

pub(crate) struct PlayerGameplayHost;

impl NativeGameplayHost for PlayerGameplayHost {
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        player::prepare_native_play_policies(policies)
    }
    fn prepare_policies(
        &mut self,
        policies: &[(PlayerId, &crate::gauge::GaugeProfile)],
    ) -> NativeGameplayResult<()> {
        player::prepare_native_policies(policies)
    }
    fn cancelled(&self) -> bool {
        player::cancelled()
    }
    fn pause_requested(&self) -> bool {
        player::pause_requested()
    }
    fn retry_pause_publication(&mut self) {
        player::retry_pause_publication();
    }
    fn publish_pause(&mut self, pause: PauseState) {
        player::publish_pause(pause);
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        player::publish_section_end(end);
    }
    fn advertise_practice(
        &mut self,
        capability: Option<crate::practice_control::PracticeCapability>,
    ) -> NativeGameplayResult<()> {
        Ok(player::advertise_practice(capability)?)
    }
    fn take_practice_request(
        &mut self,
    ) -> NativeGameplayResult<Option<crate::practice_control::PracticeRequest>> {
        Ok(player::take_practice_request()?)
    }
    fn commit_practice_reply(
        &mut self,
        reply: &crate::practice_control::PracticeReply,
    ) -> NativeGameplayResult<()> {
        Ok(player::commit_practice_reply(reply)?)
    }
    fn apply_practice_identity(
        &mut self,
        prepared: &mut player::PreparedPracticePresentation,
        generation: u64,
    ) {
        player::apply_practice_identity(prepared, generation);
    }
    fn commit_practice_presentation(
        &mut self,
        prepared: player::PreparedPracticePresentation,
        generation: u64,
    ) -> NativeGameplayResult<()> {
        player::commit_practice_presentation(prepared, generation)
    }
    fn prepare_practice_presentation(
        &mut self,
        generation: u64,
        attempts: &[(PlayerId, &crate::practice_session::PreparedPracticeAttempt)],
    ) -> NativeGameplayResult<player::PreparedPracticePresentation> {
        player::prepare_practice_presentation(generation, attempts)
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        player::publish_report(report)
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        player::publish_local_reports(reports)
    }
    fn publish_completed_solo(&mut self, result: CompletedPlayResult) -> NativeGameplayResult<()> {
        player::publish_completed_solo(result)
    }
    fn publish_completed_local(
        &mut self,
        results: &[(PlayerId, CompletedPlayResult)],
    ) -> NativeGameplayResult<()> {
        player::publish_completed_local(results)
    }
    fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>) {
        match diagnostic {
            NativeGameplayDiagnostic::SoloReport(report) => {
                for event in &report.judge_events {
                    println!("judge={event:?}");
                }
                if !report.audio_failures.is_empty() {
                    eprintln!("exact failed audio commands={:?}", report.audio_failures);
                }
            }
            NativeGameplayDiagnostic::LocalReport { player, report } => {
                for event in &report.judge_events {
                    println!("player{} judge={event:?}", player.0);
                }
            }
            NativeGameplayDiagnostic::Pause { local, boundary } => {
                let owner = if local { "local" } else { "live" };
                println!(
                    "{owner} pause={} host window={:?}, software cutoff={:?}, exact song={:?}; acoustic accuracy unmeasured",
                    boundary.paused, boundary.window, boundary.at, boundary.song,
                );
            }
            NativeGameplayDiagnostic::Discipline {
                base_rate_ppm,
                correction_ppm,
                applied_rate_ppm,
                phase_error_ns,
                limited,
                quality,
            } => println!(
                "discipline measured={base_rate_ppm:+}ppm correction={correction_ppm:+}ppm applied={applied_rate_ppm:+}ppm phase={phase_error_ns}ns limited={limited} quality={quality:?}",
            ),
            NativeGameplayDiagnostic::SongProgress(song) => {
                println!("logical song={}ns", song.as_nanos());
            }
        }
    }
}

/// Existing native entry point, composed with system waiting and player output.
pub fn run_gameplay<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    run_gameplay_with_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
    )
}

/// Compatibility entry point with explicit control and legacy player output.
pub fn run_gameplay_with_control<D: NativeGameplayDevice, C: NativePumpControl>(
    device: &mut D,
    session: NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
    control: &mut C,
) -> NativeGameplayResult<()> {
    run_gameplay_with_ports(device, session, config, control, &mut PlayerGameplayHost)
}

/// Existing cohort entry point, composed outside the shared gameplay policy.
pub fn run_cohort<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeCohortSession<'_>,
    config: NativeGameplayConfig,
) -> NativeGameplayResult<()> {
    run_cohort_with_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
    )
}

/// Compatibility cohort entry point with explicit control and player output.
pub fn run_cohort_with_control<D: NativeGameplayDevice, C: NativePumpControl>(
    device: &mut D,
    session: NativeCohortSession<'_>,
    config: NativeGameplayConfig,
    control: &mut C,
) -> NativeGameplayResult<()> {
    run_cohort_with_ports(device, session, config, control, &mut PlayerGameplayHost)
}

/// Returns actual completion evidence independently of native cleanup status.
pub fn run_gameplay_with_result<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    run_gameplay_with_result_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
    )
}

pub fn run_gameplay_with_result_and_score<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
    score: &mut crate::competition::ScoreSummary,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    crate::native_gameplay::run_gameplay_with_result_and_score_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
        score,
    )
}

pub fn run_cohort_with_results<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeCohortSession<'_>,
    config: NativeGameplayConfig,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    run_cohort_with_results_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
    )
}

/// Static cold-setup interceptor; report/publication effects stay with the original host.
pub(crate) struct ResolvedGameplayHost<'a, 'p, H> {
    pub(crate) host: &'a mut H,
    pub(crate) policies: &'a [(PlayerId, &'p crate::play_policy::ResolvedPlayPolicy)],
}
impl<H: NativeGameplayHost> NativeGameplayHost for ResolvedGameplayHost<'_, '_, H> {
    fn prepare_policies(
        &mut self,
        gauges: &[(PlayerId, &crate::gauge::GaugeProfile)],
    ) -> NativeGameplayResult<()> {
        if !gauges
            .iter()
            .zip(self.policies)
            .all(|((id, gauge), (selected_id, policy))| {
                id == selected_id && *gauge == policy.gauge()
            })
            || gauges.len() != self.policies.len()
        {
            return Err("native pump preparation differs from selected policy roster".into());
        }
        self.host.prepare_play_policies(self.policies)
    }
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        self.host.prepare_play_policies(policies)
    }
    fn cancelled(&self) -> bool {
        self.host.cancelled()
    }
    fn pause_requested(&self) -> bool {
        self.host.pause_requested()
    }
    fn retry_pause_publication(&mut self) {
        self.host.retry_pause_publication();
    }
    fn publish_pause(&mut self, pause: PauseState) {
        self.host.publish_pause(pause);
    }
    fn publish_section_end(&mut self, end: Timestamp) {
        self.host.publish_section_end(end);
    }
    fn advertise_practice(
        &mut self,
        capability: Option<crate::practice_control::PracticeCapability>,
    ) -> NativeGameplayResult<()> {
        self.host.advertise_practice(capability)
    }
    fn take_practice_request(
        &mut self,
    ) -> NativeGameplayResult<Option<crate::practice_control::PracticeRequest>> {
        self.host.take_practice_request()
    }
    fn commit_practice_reply(
        &mut self,
        reply: &crate::practice_control::PracticeReply,
    ) -> NativeGameplayResult<()> {
        self.host.commit_practice_reply(reply)
    }
    fn apply_practice_identity(
        &mut self,
        prepared: &mut player::PreparedPracticePresentation,
        generation: u64,
    ) {
        self.host.apply_practice_identity(prepared, generation);
    }
    fn commit_practice_presentation(
        &mut self,
        prepared: player::PreparedPracticePresentation,
        generation: u64,
    ) -> NativeGameplayResult<()> {
        self.host.commit_practice_presentation(prepared, generation)
    }
    fn prepare_practice_presentation(
        &mut self,
        generation: u64,
        attempts: &[(PlayerId, &crate::practice_session::PreparedPracticeAttempt)],
    ) -> NativeGameplayResult<player::PreparedPracticePresentation> {
        self.host
            .prepare_practice_presentation(generation, attempts)
    }
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        self.host.publish_report(report)
    }
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()> {
        self.host.publish_local_reports(reports)
    }
    fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>) {
        self.host.diagnostic(diagnostic);
    }
    fn publish_completed_solo(&mut self, result: CompletedPlayResult) -> NativeGameplayResult<()> {
        self.host.publish_completed_solo(result)
    }
    fn publish_completed_local(
        &mut self,
        results: &[(PlayerId, CompletedPlayResult)],
    ) -> NativeGameplayResult<()> {
        self.host.publish_completed_local(results)
    }
}

pub fn run_gameplay_with_policy_and_result_and_score<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeGameplaySession<'_>,
    config: NativeGameplayConfig,
    score: &mut crate::competition::ScoreSummary,
    policy: &crate::play_policy::ResolvedPlayPolicy,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    crate::native_gameplay::run_gameplay_with_policy_and_result_and_score_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
        score,
        policy,
    )
}

pub fn run_cohort_with_policies_and_results<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeCohortSession<'_>,
    config: NativeGameplayConfig,
    policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_cohort::run_cohort_with_policies_and_results_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
        policies,
    )
}

/// Native solo composition using one audio authority and original HOST merger.
pub type NativeAudioGameplaySession<'a> =
    crate::native_gameplay::AudioGameplaySession<'a, LiveCompetition>;
/// Native local composition using one shared audio authority.
pub type NativeAudioCohortSession<'a> =
    crate::native_cohort::AudioCohortSession<'a, LiveCompetition, NativeGroupCompetition>;

/// Runs actual native audio-authoritative solo play and returns completion evidence.
pub fn run_gameplay_audio_with_result<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeAudioGameplaySession<'_>,
    config: crate::native_gameplay::AudioGameplayConfig,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    crate::native_gameplay::run_gameplay_audio_with_result_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
    )
}
/// Runs selected native policy with actual accepted-score observation.
pub fn run_gameplay_audio_with_policy_and_result_and_score<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeAudioGameplaySession<'_>,
    config: crate::native_gameplay::AudioGameplayConfig,
    score: &mut crate::competition::ScoreSummary,
    policy: &crate::play_policy::ResolvedPlayPolicy,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    crate::native_gameplay::run_gameplay_audio_with_policy_and_result_and_score_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
        score,
        policy,
    )
}
/// Runs the native cohort with original member identity and one output authority.
pub fn run_cohort_audio_with_results<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeAudioCohortSession<'_>,
    config: crate::native_gameplay::AudioGameplayConfig,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_cohort::run_cohort_audio_with_results_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
    )
}
/// Runs selected member policies on the native audio-authoritative cohort.
pub fn run_cohort_audio_with_policies_and_results<D: NativeGameplayDevice>(
    device: &mut D,
    session: NativeAudioCohortSession<'_>,
    config: crate::native_gameplay::AudioGameplayConfig,
    policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_cohort::run_cohort_audio_with_policies_and_results_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
        policies,
    )
}

/// Input composition shared by native startup and gameplay output owners.
/// Cold native file effect for genuine per-attempt captures.
pub struct NativePracticeRecorder<F> {
    save: F,
}
impl<F> NativePracticeRecorder<F> {
    pub fn new(save: F) -> Self {
        Self { save }
    }
}
impl<F> crate::practice_playback::PracticeRecordingPort for NativePracticeRecorder<F>
where
    F: FnMut(
        Option<crate::replay_capture::LiveReplayCapture>,
        Option<&std::path::Path>,
        bool,
    ) -> NativeGameplayResult<()>,
{
    fn archive(
        &mut self,
        _: PlayerId,
        path: &std::path::Path,
        capture: crate::replay_capture::LiveReplayCapture,
    ) -> NativeGameplayResult<()> {
        (self.save)(Some(capture), Some(path), true)
    }
}

/// Selected solo practice through the existing native device and UI adapters.
pub fn run_gameplay_audio_with_practice_and_result_and_score<
    D: NativeGameplayDevice,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    session: NativeAudioGameplaySession<'_>,
    config: crate::native_gameplay::AudioGameplayConfig,
    score: &mut crate::competition::ScoreSummary,
    policy: &crate::play_policy::ResolvedPlayPolicy,
    practice: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
) -> NativeGameplayResult<Option<CompletedPlayResult>> {
    crate::native_policy_admission::validate_audio_config(&config)?;
    if *score != crate::competition::ScoreSummary::default() {
        return Err("practice requires a fresh initial score".into());
    }
    let epoch = session.session.discipline.authority().epoch();
    let header = session
        .session
        .competition
        .as_ref()
        .filter(|port| !port.policy_agnostic())
        .map(|port| {
            port.expected_policy_header()
                .ok_or("practice competition has no pinned policy header")
        })
        .transpose()?;
    crate::native_policy_admission::validate_selected_in_domain(
        session.session.runtime.judge(),
        session.session.gauge,
        policy,
        session.session.capture.as_ref(),
        header,
        &config.gameplay,
        epoch.logical_origin.domain,
        Some(config.section_start),
    )?;
    let policies = [(PlayerId(1), policy)];
    let mut host = PlayerGameplayHost;
    if policy.gauge() == &crate::gauge::GaugeProfile::default() {
        host.prepare_play_policies(&policies)?;
    }
    let mut resolved = ResolvedGameplayHost {
        host: &mut host,
        policies: &policies,
    };
    let mut scored = crate::native_gameplay_host::NativeScoreHost::new(&mut resolved, score);
    crate::native_gameplay::run_gameplay_audio_with_practice_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut scored,
        practice,
        recording,
    )
}

/// Selected local cohort practice on the existing shared native output owner.
pub fn run_cohort_audio_with_practice_and_policies_and_results<
    D: NativeGameplayDevice,
    R: crate::practice_playback::PracticeRecordingPort,
>(
    device: &mut D,
    session: NativeAudioCohortSession<'_>,
    config: crate::native_gameplay::AudioGameplayConfig,
    policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    practice: &mut crate::practice_playback::PracticePlayback,
    recording: &mut R,
) -> NativeGameplayResult<Option<Vec<(PlayerId, CompletedPlayResult)>>> {
    crate::native_cohort::run_cohort_audio_practice_with_policies_and_results_and_ports(
        device,
        session,
        config,
        &mut SystemControl,
        &mut PlayerGameplayHost,
        policies,
        practice,
        recording,
    )
}

/// Input composition shared by native startup and gameplay output owners.
/// The same buffer and collector must survive the phase handoff.
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeCollectedInput {
    retained: VecDeque<PhysicalInputEvent>,
    completed_through: Option<ClockPoint>,
}
#[cfg(not(target_arch = "wasm32"))]
impl NativeCollectedInput {
    pub fn new() -> NativeGameplayResult<Self> {
        let mut retained = VecDeque::new();
        retained.try_reserve_exact(crate::native_start::MAX_START_INPUT_EVENTS + 1)?;
        Ok(Self {
            retained,
            completed_through: None,
        })
    }
    pub fn service_start(
        &mut self,
        collector: &mut crate::native_input::NativeInputCollector,
        retain: bool,
        pre_origin: &mut u64,
        max_items: usize,
    ) -> NativeGameplayResult<bool> {
        // Discarded setup input is counted exactly as in the synchronous owners.
        // Retained input and its FIFO cut stay together until gameplay acquisition.
        let room = crate::native_start::MAX_START_INPUT_EVENTS.saturating_sub(self.retained.len());
        let batch = collector.drain(&mut self.retained, max_items.min(room + 1))?;
        if let Some(cut) = batch.completed_through {
            self.completed_through = Some(cut);
        }
        if !retain {
            *pre_origin = pre_origin.saturating_add(self.retained.len() as u64);
            self.retained.clear();
        } else if self.retained.len() > crate::native_start::MAX_START_INPUT_EVENTS {
            return Err("startup input capacity exceeded; restart required".into());
        }
        Ok(!batch.closed)
    }
    pub fn acquire(
        &mut self,
        collector: &mut crate::native_input::NativeInputCollector,
        events: &mut VecDeque<PhysicalInputEvent>,
        max_items: usize,
    ) -> NativeGameplayResult<InputBatch> {
        let limit = crate::native_gameplay::MAX_PENDING_INPUT_EVENTS;
        if events.len().saturating_add(self.retained.len()) > limit {
            return Err("native pending input capacity exceeded; restart required".into());
        }
        let transferred_retained = !self.retained.is_empty() || self.completed_through.is_some();
        events.try_reserve(self.retained.len())?;
        events.append(&mut self.retained);
        let allowance = max_items.min(limit - events.len());
        let batch = collector.drain(events, allowance)?;
        let cut = batch.completed_through.or(self.completed_through.take());
        self.completed_through = None;
        Ok(InputBatch {
            backlog: batch.backlog,
            closed: batch.closed && !transferred_retained,
            completed_through: cut,
        })
    }
}
