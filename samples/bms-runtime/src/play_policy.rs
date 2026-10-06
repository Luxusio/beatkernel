//! Control-side resolution of one immutable judge/gauge setup; no native IO.
use crate::gauge::{GaugeError, GaugeProfile, MAX_GAUGE_GRADES};
use beatkernel::{
    judge::{JudgeError, JudgeGrade, JudgeProfile, JudgeWindow},
    time::Duration,
};
use beatkernel_bms::{BmsChart, BmsGaugeError, BmsGaugeKind, BmsJudgment, ResolvedTotal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaugeSelection {
    BeatKernel,
    Bms(BmsGaugeKind),
}
impl std::str::FromStr for GaugeSelection {
    type Err = PolicyError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "beatkernel" => Self::BeatKernel,
            "assist-easy" => Self::Bms(BmsGaugeKind::AssistEasy),
            "easy" => Self::Bms(BmsGaugeKind::Easy),
            "groove" => Self::Bms(BmsGaugeKind::Groove),
            "hard" => Self::Bms(BmsGaugeKind::Hard),
            "ex-hard" => Self::Bms(BmsGaugeKind::ExHard),
            "hazard" => Self::Bms(BmsGaugeKind::Hazard),
            _ => return Err(PolicyError::Invalid("unknown gauge selection")),
        })
    }
}
/// Original chart statistics retained before practice heads are removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginalGaugeContext {
    total: ResolvedTotal,
    stages: u64,
}
impl OriginalGaugeContext {
    pub fn from_source(source: &BmsChart) -> Self {
        Self {
            total: source.gauge_total(),
            stages: source.judged_stage_count(),
        }
    }
    pub const fn total(&self) -> ResolvedTotal {
        self.total
    }
    pub const fn judged_stages(&self) -> u64 {
        self.stages
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassifiedWindow {
    pub judgment: BmsJudgment,
    pub window: JudgeWindow,
}
#[derive(Debug)]
pub enum PolicyError {
    Invalid(&'static str),
    AllocationFailed,
    Judge(JudgeError),
    Gauge(GaugeError),
    Bms(BmsGaugeError),
}
impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "play policy: {self:?}")
    }
}
impl std::error::Error for PolicyError {}
#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedPlayPolicy {
    selection: GaugeSelection,
    judge: JudgeProfile,
    gauge: GaugeProfile,
    total: Option<ResolvedTotal>,
}
impl ResolvedPlayPolicy {
    pub fn builtin(early: i64, late: i64, offset: i64) -> Result<Self, PolicyError> {
        let mut windows = Vec::new();
        windows
            .try_reserve_exact(1)
            .map_err(|_| PolicyError::AllocationFailed)?;
        windows.push(JudgeWindow {
            grade: JudgeGrade(1),
            early: Duration::from_nanos(early),
            late: Duration::from_nanos(late),
        });
        Ok(Self {
            selection: GaugeSelection::BeatKernel,
            judge: JudgeProfile::new(windows, Duration::from_nanos(offset))
                .map_err(PolicyError::Judge)?,
            gauge: GaugeProfile::default(),
            total: None,
        })
    }
    /// Resolve from the original source; callers retain this policy when selecting practice heads.
    /// Every hit grade is explicitly classified; misses map to POOR.
    pub fn bms(
        source: &BmsChart,
        kind: BmsGaugeKind,
        classified: &[ClassifiedWindow],
        offset: i64,
    ) -> Result<Self, PolicyError> {
        Self::from_context(
            &OriginalGaugeContext::from_source(source),
            kind,
            classified,
            offset,
        )
    }
    pub fn from_context(
        context: &OriginalGaugeContext,
        kind: BmsGaugeKind,
        classified: &[ClassifiedWindow],
        offset: i64,
    ) -> Result<Self, PolicyError> {
        if classified.is_empty() || classified.len() > MAX_GAUGE_GRADES {
            return Err(PolicyError::Invalid(
                "hit windows must contain 1..=64 grades",
            ));
        }
        if classified
            .iter()
            .any(|entry| matches!(entry.judgment, BmsJudgment::Poor | BmsJudgment::EmptyPoor))
        {
            return Err(PolicyError::Invalid(
                "poor classes cannot classify a hit window",
            ));
        }
        let mut windows = Vec::new();
        let mut grades = Vec::new();
        windows
            .try_reserve_exact(classified.len())
            .map_err(|_| PolicyError::AllocationFailed)?;
        grades
            .try_reserve_exact(classified.len())
            .map_err(|_| PolicyError::AllocationFailed)?;
        windows.extend(classified.iter().map(|entry| entry.window));
        grades.extend(
            classified
                .iter()
                .map(|entry| (entry.window.grade, entry.judgment)),
        );
        let judge =
            JudgeProfile::new(windows, Duration::from_nanos(offset)).map_err(PolicyError::Judge)?;
        let total = context.total;
        let rules = beatkernel_bms::BmsGaugeRules::lr2(kind, total.total, context.stages)
            .map_err(PolicyError::Bms)?;
        let gauge = GaugeProfile::from_bms_rules(rules, BmsJudgment::PGreat, &grades)
            .map_err(PolicyError::Gauge)?;
        Ok(Self {
            selection: GaugeSelection::Bms(kind),
            judge,
            gauge,
            total: Some(total),
        })
    }
    pub const fn selection(&self) -> GaugeSelection {
        self.selection
    }
    pub fn judge(&self) -> &JudgeProfile {
        &self.judge
    }
    pub fn gauge(&self) -> &GaugeProfile {
        &self.gauge
    }
    pub const fn total(&self) -> Option<ResolvedTotal> {
        self.total
    }
    pub fn into_parts(self) -> (JudgeProfile, GaugeProfile) {
        (self.judge, self.gauge)
    }
}
#[cfg(test)]
#[path = "play_policy_fixtures.rs"]
mod fixtures;
