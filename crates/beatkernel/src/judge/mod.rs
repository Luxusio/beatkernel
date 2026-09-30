//! Configurable deterministic judging with explicit song time and provenance.

mod engine;
mod policy;
mod profile;

pub use engine::JudgeEngine;
pub use policy::{
    Candidate, CandidateResolver, ClosestCandidate, EarliestCandidate, JudgePolicy,
    WindowJudgePolicy,
};
pub use profile::{JudgeGrade, JudgeProfile, JudgeWindow};

use std::fmt;

use crate::{
    chart::{InteractionId, ObjectId},
    input::{EventMeta, GameControlId},
    interaction::InteractionEvaluator,
    time::{Duration, Timestamp},
};

/// A separately reported stage of an interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JudgeStage {
    /// The sole stage of a point interaction.
    Instant,
    /// Acquisition of a held interaction.
    HoldHead,
    /// Release or failure of a held interaction.
    HoldTail,
    /// A caller-defined stage identity.
    Custom(u32),
}

/// Why an interaction stage failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MissReason {
    /// No accepted start arrived before the head deadline expired.
    HeadTimeout,
    /// No accepted release arrived before the tail deadline expired.
    TailTimeout,
    /// The owner released before an accepted tail window.
    EarlyRelease,
    /// The grading policy rejected an otherwise eligible owner release.
    RejectedInput,
}

/// A structured grade or explicit failure for one stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JudgeOutcome {
    /// An accepted input with its signed effective-time error.
    Hit {
        /// Caller-defined grade.
        grade: JudgeGrade,
        /// Effective input time minus stage target.
        delta: Duration,
    },
    /// A stage failed without a grade.
    Miss {
        /// Explicit failure classification.
        reason: MissReason,
    },
}

/// An engine-stamped result retaining input provenance when input caused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgeEvent {
    /// Chart-local object identity.
    pub object: ObjectId,
    /// Reported interaction stage.
    pub stage: JudgeStage,
    /// Grade or miss.
    pub outcome: JudgeOutcome,
    /// Effective song time at which the result was emitted.
    pub at: Timestamp,
    /// Original input metadata; absent for timeouts.
    pub input: Option<EventMeta>,
}

/// Caller registration of an opaque interaction ID and its logical destination.
pub struct Rule {
    /// Game-defined interaction identity used by compiled objects.
    pub interaction: InteractionId,
    /// Logical game control or stream channel.
    pub control: GameControlId,
    /// Validates and creates each object's active evaluator.
    pub evaluator: Box<dyn InteractionEvaluator>,
}

/// Library-owned configuration or operation errors, before state mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JudgeError {
    /// Windows are empty, negative, shrinking, or have duplicate grades.
    InvalidProfile,
    /// A compiled object refers to an unregistered interaction.
    UnknownInteraction {
        /// Missing interaction identity.
        id: InteractionId,
    },
    /// An interaction identity has more than one rule.
    DuplicateRule {
        /// Repeated interaction identity.
        id: InteractionId,
    },
    /// An object range is incompatible with its evaluator.
    InvalidObjectRange {
        /// Invalid chart-local object identity.
        object: ObjectId,
    },
    /// Effective song time regressed behind the last accepted operation.
    NonMonotonicSongTime,
    /// A required timestamp or result duration cannot be represented.
    Overflow,
    /// The resolver selected an identity outside the eligible candidates.
    InvalidCandidate {
        /// Rejected object identity.
        object: ObjectId,
    },
}

impl fmt::Display for JudgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProfile => formatter.write_str(
                "judge windows must be nonempty, nonnegative, nested and have unique grades",
            ),
            Self::UnknownInteraction { id } => write!(formatter, "unknown interaction ID {}", id.0),
            Self::DuplicateRule { id } => {
                write!(formatter, "duplicate interaction rule ID {}", id.0)
            }
            Self::InvalidObjectRange { object } => write!(
                formatter,
                "invalid interaction range for object ID {}",
                object.0
            ),
            Self::NonMonotonicSongTime => {
                formatter.write_str("effective judge song time must not regress")
            }
            Self::Overflow => formatter.write_str("judge time or duration overflow"),
            Self::InvalidCandidate { object } => write!(
                formatter,
                "resolver selected ineligible object ID {}",
                object.0
            ),
        }
    }
}

impl std::error::Error for JudgeError {}
