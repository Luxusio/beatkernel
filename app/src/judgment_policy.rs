//! Explicit BMS grade meaning and checked score projection, without native IO.
use crate::{competition::ScoreSummary, gauge::MAX_GAUGE_GRADES};
use beatkernel::judge::{JudgeGrade, JudgeProfile};
use beatkernel_bms::BmsJudgment;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradeClass {
    pub grade: JudgeGrade,
    pub class: BmsJudgment,
}
const UNUSED: GradeClass = GradeClass {
    grade: JudgeGrade(0),
    class: BmsJudgment::Bad,
};

/// Cold immutable bounded mapping. Neither construction nor lookup allocates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsJudgmentPolicy {
    entries: [GradeClass; MAX_GAUGE_GRADES],
    len: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JudgmentPolicyError {
    InvalidClasses,
    ProfileMismatch,
    UnknownGrade,
    InconsistentScore,
    Overflow,
}
impl std::fmt::Display for JudgmentPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BMS judgment policy: {self:?}")
    }
}
impl std::error::Error for JudgmentPolicyError {}

/// Classified actual judged stages. Empty presses are deliberately absent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BmsScoreSummary {
    pub pgreat: u64,
    pub great: u64,
    pub good: u64,
    pub bad: u64,
    pub poor: u64,
    pub ex_score: u64,
}
impl BmsScoreSummary {
    /// Checks a prepared summary at a presentation boundary without reading grades.
    pub fn validate_for(&self, hits: u64, misses: u64) -> Result<(), JudgmentPolicyError> {
        let sum = self
            .pgreat
            .checked_add(self.great)
            .and_then(|n| n.checked_add(self.good))
            .and_then(|n| n.checked_add(self.bad))
            .ok_or(JudgmentPolicyError::Overflow)?;
        let ex = self
            .pgreat
            .checked_mul(2)
            .and_then(|n| n.checked_add(self.great))
            .ok_or(JudgmentPolicyError::Overflow)?;
        hits.checked_add(misses)
            .ok_or(JudgmentPolicyError::Overflow)?;
        if sum != hits || self.poor != misses || self.ex_score != ex {
            return Err(JudgmentPolicyError::InconsistentScore);
        }
        Ok(())
    }
}
impl BmsJudgmentPolicy {
    pub fn new(entries: &[GradeClass]) -> Result<Self, JudgmentPolicyError> {
        if entries.is_empty()
            || entries.len() > MAX_GAUGE_GRADES
            || entries
                .iter()
                .any(|entry| matches!(entry.class, BmsJudgment::Poor | BmsJudgment::EmptyPoor))
        {
            return Err(JudgmentPolicyError::InvalidClasses);
        }
        let mut policy = Self {
            entries: [UNUSED; MAX_GAUGE_GRADES],
            len: entries.len(),
        };
        policy.entries[..policy.len].copy_from_slice(entries);
        policy.entries[..policy.len].sort_unstable_by_key(|entry| entry.grade.0);
        if policy
            .entries()
            .windows(2)
            .any(|pair| pair[0].grade == pair[1].grade)
        {
            return Err(JudgmentPolicyError::InvalidClasses);
        }
        Ok(policy)
    }
    pub fn entries(&self) -> &[GradeClass] {
        &self.entries[..self.len]
    }
    pub fn class(&self, grade: JudgeGrade) -> Option<BmsJudgment> {
        self.entries()
            .binary_search_by_key(&grade.0, |entry| entry.grade.0)
            .ok()
            .map(|index| self.entries[index].class)
    }
    pub fn validate_profile(&self, profile: &JudgeProfile) -> Result<(), JudgmentPolicyError> {
        if profile.windows().len() != self.len
            || profile
                .windows()
                .iter()
                .any(|window| self.class(window.grade).is_none())
        {
            return Err(JudgmentPolicyError::ProfileMismatch);
        }
        Ok(())
    }
    /// Projects an already committed prefix without changing it or guessing grades.
    pub fn project(&self, score: &ScoreSummary) -> Result<BmsScoreSummary, JudgmentPolicyError> {
        self.project_counts(
            score.hits,
            score.misses,
            score.grades.iter().map(|(&grade, &count)| (grade, count)),
        )
    }
    /// Shared checked projection over already validated grade storage.
    pub(crate) fn project_counts(
        &self,
        expected_hits: u64,
        misses: u64,
        grades: impl Iterator<Item = (u32, u64)>,
    ) -> Result<BmsScoreSummary, JudgmentPolicyError> {
        let mut result = BmsScoreSummary {
            poor: misses,
            ..Default::default()
        };
        let mut hits = 0u64;
        for (grade, count) in grades {
            let target = match self
                .class(JudgeGrade(grade))
                .ok_or(JudgmentPolicyError::UnknownGrade)?
            {
                BmsJudgment::PGreat => &mut result.pgreat,
                BmsJudgment::Great => &mut result.great,
                BmsJudgment::Good => &mut result.good,
                BmsJudgment::Bad => &mut result.bad,
                _ => unreachable!("construction restricts hit classes"),
            };
            *target = target
                .checked_add(count)
                .ok_or(JudgmentPolicyError::Overflow)?;
            hits = hits
                .checked_add(count)
                .ok_or(JudgmentPolicyError::Overflow)?;
        }
        if hits != expected_hits {
            return Err(JudgmentPolicyError::InconsistentScore);
        }
        hits.checked_add(misses)
            .ok_or(JudgmentPolicyError::Overflow)?;
        result.ex_score = result
            .pgreat
            .checked_mul(2)
            .and_then(|points| points.checked_add(result.great))
            .ok_or(JudgmentPolicyError::Overflow)?;
        Ok(result)
    }
}
