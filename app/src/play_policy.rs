//! Control-side resolution of one immutable judge/gauge setup; no native IO.
use crate::gauge::{GaugeError, GaugeProfile, MAX_GAUGE_GRADES};
use beatkernel::{
    judge::{JudgeError, JudgeGrade, JudgeProfile, JudgeWindow},
    time::Duration,
};
use beatkernel_bms::{
    BmsChart, BmsGaugeError, BmsGaugeKind, BmsJudgeDifficulty, BmsJudgment, BmsRankPrecedence,
    BmsTimingPreset, BmsTimingPresetError, BmsTimingProfiles, BmsTimingStage, ResolvedTotal,
};

/// Explicit numerical version and header precedence; neither has a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingPresetSelection {
    pub preset: BmsTimingPreset,
    pub precedence: BmsRankPrecedence,
}

/// Immutable selected numerical policy, retaining the caller's precedence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTimingPolicy {
    selection: TimingPresetSelection,
    profiles: BmsTimingProfiles,
}
impl ResolvedTimingPolicy {
    /// Recomputes a recorded numerical policy from its explicit typed declaration.
    /// Codecs must additionally compare every stored effective window and version.
    pub fn from_recorded(
        selection: TimingPresetSelection,
        difficulty: BmsJudgeDifficulty,
    ) -> Result<Self, PolicyError> {
        Ok(Self {
            selection,
            profiles: selection
                .preset
                .resolve(difficulty)
                .map_err(PolicyError::Timing)?,
        })
    }
    /// Verifies actual immutable stages and the selected indexed head envelope.
    pub fn validate_judge(
        &self,
        judge: &beatkernel::judge::JudgeEngine,
        mode: beatkernel_bms::BmsInputMode,
    ) -> Result<(), PolicyError> {
        let expected = self
            .profiles
            .head_envelope(judge.profile().input_offset())
            .map_err(PolicyError::Judge)?;
        if &expected != judge.profile() {
            return Err(PolicyError::Invalid(
                "selected timing routing envelope differs",
            ));
        }
        let profiles = [
            BmsTimingStage::KeyHead,
            BmsTimingStage::ScratchHead,
            BmsTimingStage::KeyTail,
            BmsTimingStage::ScratchTail,
        ]
        .map(|stage| self.profiles.judge_profile(stage));
        let [key_head, scratch_head, key_tail, scratch_tail] = profiles;
        let (key_head, scratch_head, key_tail, scratch_tail) = (
            key_head.map_err(PolicyError::Judge)?,
            scratch_head.map_err(PolicyError::Judge)?,
            key_tail.map_err(PolicyError::Judge)?,
            scratch_tail.map_err(PolicyError::Judge)?,
        );
        for object in judge.chart().objects() {
            let actual = judge.builtin_timing(object.id).ok_or(PolicyError::Invalid(
                "selected timing requires staged builtin rules",
            ))?;
            // BMS logical controls preserve the visible source channel code.
            if !matches!(actual.control.0, 0x11..=0x19 | 0x21..=0x29) {
                return Err(PolicyError::Invalid(
                    "selected timing has an unknown BMS lane",
                ));
            }
            let (head, tail) = if actual.control.0 & 15 == 6 {
                (&scratch_head, &scratch_tail)
            } else {
                (&key_head, &key_tail)
            };
            if actual.head != head
                || actual.tail != object.time.end.map(|_| tail)
                || actual.accepts_contact != (mode == beatkernel_bms::BmsInputMode::ButtonOrContact)
                || actual.semantics != self.interaction_semantics()
            {
                return Err(PolicyError::Invalid(
                    "selected timing differs from actual stage rules",
                ));
            }
        }
        Ok(())
    }
    pub const fn selection(&self) -> TimingPresetSelection {
        self.selection
    }
    pub fn profiles(&self) -> &BmsTimingProfiles {
        &self.profiles
    }
    /// Recorded independently from the numerical-table version.
    pub const fn interaction_semantics(&self) -> &'static str {
        "beatkernel-hold/v1"
    }
}

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
    Rank(beatkernel_bms::BmsError),
    Timing(BmsTimingPresetError),
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
    judgments: Option<crate::judgment_policy::BmsJudgmentPolicy>,
    timing: Option<ResolvedTimingPolicy>,
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
            judgments: None,
            timing: None,
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
        let mut entries = [crate::judgment_policy::GradeClass {
            grade: JudgeGrade(0),
            class: BmsJudgment::Bad,
        }; MAX_GAUGE_GRADES];
        for (entry, classified) in entries.iter_mut().zip(classified) {
            *entry = crate::judgment_policy::GradeClass {
                grade: classified.window.grade,
                class: classified.judgment,
            };
        }
        let judgments =
            crate::judgment_policy::BmsJudgmentPolicy::new(&entries[..classified.len()])
                .map_err(|_| PolicyError::Invalid("invalid judgment classes"))?;
        Ok(Self {
            selection: GaugeSelection::Bms(kind),
            judge,
            gauge,
            total: Some(total),
            judgments: Some(judgments),
            timing: None,
        })
    }
    /// Resolves an opt-in preset from the original source before practice filtering.
    /// Hit classes use the existing ClassifiedWindow path; the judge profile is
    /// only the head routing envelope, not a replacement for staged rules.
    pub fn bms_with_timing(
        source: &BmsChart,
        kind: BmsGaugeKind,
        selection: TimingPresetSelection,
        offset: i64,
    ) -> Result<Self, PolicyError> {
        let difficulty = source
            .judge_rank_metadata()
            .map_err(PolicyError::Rank)?
            .resolve(selection.precedence)
            .ok_or(PolicyError::Invalid(
                "timing preset requires declared RANK or DEFEXRANK",
            ))?;
        let timing = ResolvedTimingPolicy::from_recorded(selection, difficulty)?;
        let profiles = &timing.profiles;
        let envelope = profiles
            .head_envelope(Duration::from_nanos(offset))
            .map_err(PolicyError::Judge)?;
        let mut classified = [ClassifiedWindow {
            judgment: BmsJudgment::PGreat,
            window: envelope.windows()[0],
        }; 4];
        for ((entry, window), class) in classified
            .iter_mut()
            .zip(envelope.windows())
            .zip(profiles.windows(BmsTimingStage::KeyHead))
        {
            *entry = ClassifiedWindow {
                judgment: class.judgment,
                window: *window,
            };
        }
        let mut policy = Self::bms(source, kind, &classified, offset)?;
        policy.timing = Some(timing);
        Ok(policy)
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
    pub fn judgments(&self) -> Option<&crate::judgment_policy::BmsJudgmentPolicy> {
        self.judgments.as_ref()
    }
    pub fn timing(&self) -> Option<&ResolvedTimingPolicy> {
        self.timing.as_ref()
    }
    /// Checks actual immutable stage windows even when no recording is enabled.
    /// Equal routing envelopes alone do not establish selected policy identity.
    pub fn validate_timing(
        &self,
        judge: &beatkernel::judge::JudgeEngine,
        mode: beatkernel_bms::BmsInputMode,
    ) -> Result<(), PolicyError> {
        let Some(timing) = &self.timing else {
            if judge
                .chart()
                .objects()
                .iter()
                .any(|object| judge.builtin_timing(object.id).is_some())
            {
                return Err(PolicyError::Invalid(
                    "staged rules require selected timing identity",
                ));
            }
            return Ok(());
        };
        if judge.profile() != &self.judge {
            return Err(PolicyError::Invalid(
                "selected timing routing envelope differs",
            ));
        }
        timing.validate_judge(judge, mode)
    }
    /// Maximum actual late stage extent, including long-note releases.
    pub fn completion_late(&self) -> Duration {
        self.timing
            .as_ref()
            .map_or(self.judge.max_late(), |timing| {
                [
                    BmsTimingStage::KeyHead,
                    BmsTimingStage::ScratchHead,
                    BmsTimingStage::KeyTail,
                    BmsTimingStage::ScratchTail,
                ]
                .into_iter()
                .flat_map(|stage| timing.profiles.windows(stage).iter())
                .map(|entry| entry.window.late)
                .max()
                .expect("four nonempty profiles")
            })
    }
    /// Legacy judge/gauge extraction does not assign class semantics to consumers.
    /// Stage windows are not carried by this legacy tuple; selected timing must
    /// use policy-aware preparation rather than rebuilding rules from the tuple.
    pub fn into_parts(self) -> (JudgeProfile, GaugeProfile) {
        (self.judge, self.gauge)
    }
}
#[cfg(test)]
#[path = "play_policy_fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[path = "timing_policy_fixtures.rs"]
mod timing_fixtures;
