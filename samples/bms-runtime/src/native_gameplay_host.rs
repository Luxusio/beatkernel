//! Explicit host commands and borrowed publication evidence for native gameplay.

use crate::{
    live_pause::LivePauseBoundary, local_players::PlayerId, local_runtime::PlayerReport,
    native_gameplay::NativeGameplayResult, play_result::CompletedPlayResult,
};
use beatkernel::{
    runtime::RuntimeReport,
    time::{ClockMappingQuality, Timestamp},
};

/// Native-owner pause capability and acknowledgement, independent of UI focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PauseState {
    #[default]
    Unavailable,
    Running,
    Pausing,
    Paused,
    Resuming,
}

/// Borrowed business evidence; formatting and terminal I/O belong to the host.
#[derive(Clone, Copy, Debug)]
pub enum NativeGameplayDiagnostic<'a> {
    SoloReport(&'a RuntimeReport),
    LocalReport {
        player: PlayerId,
        report: &'a RuntimeReport,
    },
    Pause {
        local: bool,
        boundary: LivePauseBoundary,
    },
    Discipline {
        base_rate_ppm: i64,
        correction_ppm: i64,
        applied_rate_ppm: i64,
        phase_error_ns: i128,
        limited: bool,
        quality: ClockMappingQuality,
    },
    SongProgress(Timestamp),
}

/// The application supplies commands and consumes actual committed evidence.
/// Publication refusal is a technical error, not gameplay completion or rollback.
pub trait NativeGameplayHost {
    fn cancelled(&self) -> bool;
    fn pause_requested(&self) -> bool;
    fn retry_pause_publication(&mut self);
    fn publish_pause(&mut self, pause: PauseState);
    fn publish_section_end(&mut self, end: Timestamp);
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()>;
    fn publish_local_reports(&mut self, reports: &[PlayerReport]) -> NativeGameplayResult<()>;
    fn diagnostic(&mut self, diagnostic: NativeGameplayDiagnostic<'_>);
    fn publish_completed_solo(&mut self, _: CompletedPlayResult) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn publish_completed_local(
        &mut self,
        _: &[(PlayerId, CompletedPlayResult)],
    ) -> NativeGameplayResult<()> {
        Ok(())
    }
}

/// Explicit headless operation without cancellation, publication or diagnostics.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopGameplayHost;

impl NativeGameplayHost for NoopGameplayHost {
    fn cancelled(&self) -> bool {
        false
    }
    fn pause_requested(&self) -> bool {
        false
    }
    fn retry_pause_publication(&mut self) {}
    fn publish_pause(&mut self, _: PauseState) {}
    fn publish_section_end(&mut self, _: Timestamp) {}
    fn publish_report(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn publish_local_reports(&mut self, _: &[PlayerReport]) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn diagnostic(&mut self, _: NativeGameplayDiagnostic<'_>) {}
}
