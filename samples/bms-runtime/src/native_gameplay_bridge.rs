//! Compatibility composition: legacy player publication and native pump effects.

use crate::{
    competition_live::LiveCompetition,
    gameplay_competition::{GroupCompetitionPort, SoloCompetitionPort},
    gameplay_presentation::{GameplayDevice, GameplayPresentationPort},
    live_pause::LivePauseObservation,
    native_end::{EndBoundary, NativeEnd},
    local_runtime::PlayerReport,
    multiplayer_group::MemberProgress,
    native_cohort::{
        CohortSession, GameplayPlayerState, finite_cohort_done_for_states,
        member_progress_for_states, run_cohort_with_ports, run_cohort_with_results_and_ports,
    },
    native_gameplay::{
        GameplaySession, InputBatch, NativeGameplayConfig, NativeGameplayResult,
        run_gameplay_with_ports, run_gameplay_with_result_and_ports,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_group_competition::NativeGroupCompetition,
    native_pump_control::NativePumpControl,
    native_pump_system::SystemControl,
    player,
    local_players::PlayerId,
    play_result::CompletedPlayResult,
};
use beatkernel::{
    audio::RenderReport,
    input::PhysicalInputEvent,
    runtime::RuntimeReport,
    time::{
        ClockDomainId, ClockMappingQuality, ClockPair, ClockPoint, Timestamp,
        presentation::{DisciplineConfig, DisciplineUpdate},
    },
    transport::Transport,
};

use beatkernel_platform::audio::presentation::discipline::PresentationDiscipline;
use std::collections::VecDeque;

pub trait NativeGameplayDevice {
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
    fn observe(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        LiveCompetition::observe(self, report)
    }
    fn mark_native_completed(&mut self) {
        LiveCompetition::mark_native_completed(self);
    }
}

impl GroupCompetitionPort for NativeGroupCompetition {
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
