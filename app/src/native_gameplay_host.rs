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

pub(crate) fn validate_policy_members(
    policies: &[(PlayerId, &crate::gauge::GaugeProfile)],
) -> NativeGameplayResult<()> {
    if !(1..=64).contains(&policies.len())
        || policies.iter().enumerate().any(|(index, (player, _))| {
            player.0 == 0 || policies[..index].iter().any(|(prior, _)| prior == player)
        })
    {
        return Err("native policy preparation requires 1..64 unique nonzero player IDs".into());
    }
    Ok(())
}

pub(crate) fn validate_play_policy_members(
    policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
) -> NativeGameplayResult<()> {
    if !(1..=64).contains(&policies.len())
        || policies
            .iter()
            .enumerate()
            .any(|(i, (id, _))| id.0 == 0 || policies[..i].iter().any(|(prior, _)| prior == id))
    {
        return Err("native selected policies require 1..64 unique nonzero player IDs".into());
    }
    for (_, policy) in policies {
        if let Some(classes) = policy.judgments() {
            classes.validate_profile(policy.judge())?;
            if policy.gauge() == &crate::gauge::GaugeProfile::default()
                || policy.gauge().grades().len() != classes.entries().len()
                || policy
                    .gauge()
                    .grades()
                    .iter()
                    .any(|entry| classes.class(entry.grade).is_none())
            {
                return Err("classified native policy requires a nondefault covered gauge".into());
            }
        }
    }
    Ok(())
}

/// The application supplies commands and consumes actual committed evidence.
/// Publication refusal is a technical error, not gameplay completion or rollback.
pub trait NativeGameplayHost {
    /// Cold setup capability. Legacy hosts cannot silently publish a custom policy as default.
    fn prepare_policies(
        &mut self,
        policies: &[(PlayerId, &crate::gauge::GaugeProfile)],
    ) -> NativeGameplayResult<()> {
        validate_policy_members(policies)?;
        if policies
            .iter()
            .any(|(_, profile)| **profile != crate::gauge::GaugeProfile::default())
        {
            return Err("native host does not support nondefault policy preparation".into());
        }
        Ok(())
    }
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        validate_play_policy_members(policies)?;
        if policies
            .iter()
            .any(|(_, policy)| policy.judgments().is_some())
        {
            return Err("native host does not support classified policy preparation".into());
        }
        let mut gauges = Vec::new();
        gauges.try_reserve_exact(policies.len())?;
        gauges.extend(policies.iter().map(|(id, policy)| (*id, policy.gauge())));
        self.prepare_policies(&gauges)
    }
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
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        validate_play_policy_members(policies)
    }
    fn prepare_policies(
        &mut self,
        policies: &[(PlayerId, &crate::gauge::GaugeProfile)],
    ) -> NativeGameplayResult<()> {
        validate_policy_members(policies)
    }
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

/// Static observer of genuine solo reports, retaining original publication effects.
pub struct NativeScoreHost<'a, H: NativeGameplayHost> {
    host: &'a mut H,
    score: &'a mut crate::competition::ScoreSummary,
}
impl<'a, H: NativeGameplayHost> NativeScoreHost<'a, H> {
    pub fn new(host: &'a mut H, score: &'a mut crate::competition::ScoreSummary) -> Self {
        Self { host, score }
    }
}
impl<H: NativeGameplayHost> NativeGameplayHost for NativeScoreHost<'_, H> {
    fn prepare_play_policies(
        &mut self,
        policies: &[(PlayerId, &crate::play_policy::ResolvedPlayPolicy)],
    ) -> NativeGameplayResult<()> {
        self.host.prepare_play_policies(policies)
    }
    fn prepare_policies(
        &mut self,
        policies: &[(PlayerId, &crate::gauge::GaugeProfile)],
    ) -> NativeGameplayResult<()> {
        self.host.prepare_policies(policies)
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
    fn publish_report(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()> {
        let scored = self.score.observe(&report.judge_events);
        let published = self.host.publish_report(report);
        scored?;
        published
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
#[cfg(test)]
#[path = "native_score_host_fixtures.rs"]
mod native_score_host_fixtures;
