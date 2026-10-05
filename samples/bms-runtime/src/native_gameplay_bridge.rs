//! Compatibility composition: legacy player publication and native pump effects.

use crate::{
    local_runtime::PlayerReport,
    native_cohort::{NativeCohortSession, run_cohort_with_ports},
    native_gameplay::{
        NativeGameplayConfig, NativeGameplayDevice, NativeGameplayResult, NativeGameplaySession,
        run_gameplay_with_ports,
    },
    native_gameplay_host::{NativeGameplayDiagnostic, NativeGameplayHost, PauseState},
    native_pump_control::NativePumpControl,
    native_pump_system::SystemControl,
    player,
};
use beatkernel::{runtime::RuntimeReport, time::Timestamp};

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
