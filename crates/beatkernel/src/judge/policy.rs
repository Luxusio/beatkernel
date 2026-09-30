use crate::{chart::ObjectId, time::Timestamp};

use super::{JudgeGrade, JudgeProfile};

/// Infallible caller-defined grading over the unchanged timing delta.
pub trait JudgePolicy: Send + Sync {
    /// Deep-copies all mutable policy state, or declares checkpoint unsupported.
    fn snapshot_clone(&self) -> Option<Box<dyn JudgePolicy>> {
        None
    }
    /// Stable schema identity and canonical complete policy state.
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        None
    }

    /// Returns a grade or rejects the delta without mutating judge state.
    fn grade(&self, delta: i128, profile: &JudgeProfile) -> Option<JudgeGrade>;
}

/// Grades with the first matching inclusive profile window.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowJudgePolicy;

impl JudgePolicy for WindowJudgePolicy {
    fn snapshot_clone(&self) -> Option<Box<dyn JudgePolicy>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        Some(b"WindowJudgePolicy/v1".to_vec())
    }

    fn grade(&self, delta: i128, profile: &JudgeProfile) -> Option<JudgeGrade> {
        profile.grade(delta)
    }
}

/// A pending object eligible for a particular bound input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Chart-local object identity.
    pub object: ObjectId,
    /// Start target in song time.
    pub target: Timestamp,
    /// Effective input time minus target, in wide signed nanoseconds.
    pub delta: i128,
}

/// Replaceable selection among eligible starts; declining is permitted.
pub trait CandidateResolver: Send + Sync {
    /// Deep-copies all mutable resolver state, or declares checkpoint unsupported.
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        None
    }
    /// Stable schema identity and canonical complete resolver state.
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        None
    }

    /// Returns a candidate identity, validated by the engine before mutation.
    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId>;
}

/// Selects by absolute delta, then earlier target, then object identity.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClosestCandidate;

impl CandidateResolver for ClosestCandidate {
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        Some(b"ClosestCandidate/v1".to_vec())
    }

    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        candidates
            .iter()
            .min_by_key(|candidate| {
                (
                    candidate.delta.unsigned_abs(),
                    candidate.target,
                    candidate.object,
                )
            })
            .map(|candidate| candidate.object)
    }
}

/// Selects the earliest target, breaking ties by object identity.
#[derive(Clone, Copy, Debug, Default)]
pub struct EarliestCandidate;

impl CandidateResolver for EarliestCandidate {
    fn snapshot_clone(&self) -> Option<Box<dyn CandidateResolver>> {
        Some(Box::new(*self))
    }
    fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        Some(b"EarliestCandidate/v1".to_vec())
    }

    fn select(&self, candidates: &[Candidate]) -> Option<ObjectId> {
        candidates
            .iter()
            .min_by_key(|candidate| (candidate.target, candidate.object))
            .map(|candidate| candidate.object)
    }
}
