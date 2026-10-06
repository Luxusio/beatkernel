//! Business-owned competition observations, independent of native IO owners.

use crate::{multiplayer_group::MemberProgress, native_gameplay::NativeGameplayResult};
use beatkernel::runtime::RuntimeReport;

/// Observes one player's actual committed reports and genuine completion.
pub trait SoloCompetitionPort {
    /// Only explicitly disabled observers may opt out of setup identity.
    fn policy_agnostic(&self) -> bool {
        false
    }
    fn expected_policy_header(&self) -> Option<&beatkernel::replay::ReplayHeader> {
        None
    }
    fn observe(&mut self, report: &RuntimeReport) -> NativeGameplayResult<()>;
    fn mark_native_completed(&mut self);
}

/// Observes the validated ordered prefixes of the actual local members.
pub trait GroupCompetitionPort {
    fn policy_agnostic(&self) -> bool {
        false
    }
    fn observe(&mut self, members: &[MemberProgress]) -> NativeGameplayResult<()>;
    fn mark_native_completed(&mut self);
}

/// Explicitly disabled solo competition without IO or retained state.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopSoloCompetition;

impl SoloCompetitionPort for NoopSoloCompetition {
    fn policy_agnostic(&self) -> bool {
        true
    }
    fn observe(&mut self, _: &RuntimeReport) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn mark_native_completed(&mut self) {}
}

/// Explicitly disabled group competition without IO or retained state.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopGroupCompetition;

impl GroupCompetitionPort for NoopGroupCompetition {
    fn policy_agnostic(&self) -> bool {
        true
    }
    fn observe(&mut self, _: &[MemberProgress]) -> NativeGameplayResult<()> {
        Ok(())
    }
    fn mark_native_completed(&mut self) {}
}
