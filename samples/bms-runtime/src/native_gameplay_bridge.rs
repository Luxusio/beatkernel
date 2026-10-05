//! Compatibility composition: legacy player publication and native pump effects.

use crate::{
    competition_live::LiveCompetition,
    gameplay_competition::{GroupCompetitionPort, SoloCompetitionPort},
    local_runtime::PlayerReport,
    multiplayer_group::MemberProgress,
    native_cohort::{
        CohortSession, GameplayPlayerState, finite_cohort_done_for_states,
        member_progress_for_states, run_cohort_with_ports,
    },
    native_gameplay::{
        GameplaySession, NativeGameplayConfig, NativeGameplayDevice, NativeGameplayResult,
        run_gameplay_with_ports,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_group_competition::NativeGroupCompetition,
    native_pump_control::NativePumpControl,
    native_pump_system::SystemControl,
    player,
};
use beatkernel::{
    runtime::RuntimeReport,
    time::{ClockPoint, Timestamp},
};

/// Compatibility specialization selected by native application composition.
pub type NativeGameplaySession<'a> = GameplaySession<'a, LiveCompetition>;
/// Compatibility cohort specialization with the actual native competition owners.
pub type NativeCohortSession<'a> = CohortSession<'a, LiveCompetition, NativeGroupCompetition>;
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
