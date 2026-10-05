//! Read-only classification retained after an owner's proven live completion.

use crate::gauge::{BmsGauge, GaugeFailure, GaugeSnapshot};
use beatkernel::time::Timestamp;

/// The original selected extent; unbounded nonzero starts remain practice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayResultScope {
    FullSong,
    PracticeSection {
        start: Timestamp,
        end: Option<Timestamp>,
    },
}

/// Gauge classification after completion, independent of technical owner errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayResultOutcome {
    Cleared,
    BelowClearThreshold,
    Failed(GaugeFailure),
}

/// Immutable historical evidence, constructed only by internal completed owners.
/// A gauge threshold, numeric failure or cleanup alone cannot create this result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletedPlayResult {
    scope: PlayResultScope,
    outcome: PlayResultOutcome,
    gauge: GaugeSnapshot,
}

impl CompletedPlayResult {
    pub(crate) fn from_completed(
        start: Timestamp,
        end: Option<Timestamp>,
        gauge: &BmsGauge,
    ) -> Self {
        let scope = if start == Timestamp::ZERO && end.is_none() {
            PlayResultScope::FullSong
        } else {
            PlayResultScope::PracticeSection { start, end }
        };
        let snapshot = *gauge.snapshot();
        let outcome = match snapshot.failure {
            Some(failure) => PlayResultOutcome::Failed(failure),
            None if gauge.can_clear() => PlayResultOutcome::Cleared,
            None => PlayResultOutcome::BelowClearThreshold,
        };
        Self {
            scope,
            outcome,
            gauge: snapshot,
        }
    }

    pub const fn scope(&self) -> PlayResultScope {
        self.scope
    }

    pub const fn outcome(&self) -> PlayResultOutcome {
        self.outcome
    }

    /// Exact gauge at the first proven completion, without re-observing events.
    pub const fn gauge(&self) -> GaugeSnapshot {
        self.gauge
    }

    /// Practice classification never establishes a whole-chart clear.
    pub const fn whole_song_clear(&self) -> bool {
        matches!(
            (self.scope, self.outcome),
            (PlayResultScope::FullSong, PlayResultOutcome::Cleared)
        )
    }
}
