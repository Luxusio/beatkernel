//! Fixed-point application gauge observations from actual committed reports.

use beatkernel::judge::{HazardEvent, HazardOutcome, JudgeEvent, JudgeGrade, JudgeOutcome};
use std::fmt;

/// Fixed-point units representing one percentage point.
pub const GAUGE_UNITS_PER_PERCENT: u64 = 1_000_000;
/// The clamped full gauge level, representing 100 percent.
pub const MAX_GAUGE_UNITS: u64 = 100 * GAUGE_UNITS_PER_PERCENT;
/// Maximum explicit opaque-grade overrides in one profile.
pub const MAX_GAUGE_GRADES: usize = 64;

/// An explicit hit-grade override; the grade number implies no ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GradeDelta {
    pub grade: JudgeGrade,
    pub delta: i64,
}

/// First latched gameplay failure, independent of technical owner errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaugeFailure {
    InstantDeath,
    Depleted,
}

/// Exact retained level and optional first failure reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaugeSnapshot {
    pub level_units: u64,
    pub failure: Option<GaugeFailure>,
}

/// Invalid setup or report data; rejected observations preserve the snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaugeError {
    InvalidConfiguration(&'static str),
    GradeCapacity,
    DuplicateGrade { grade: JudgeGrade },
    InvalidDamage { value: u64 },
}

impl fmt::Display for GaugeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BMS gauge: {self:?}")
    }
}
impl std::error::Error for GaugeError {}

/// Validated fixed-point policy with setup-sorted explicit grade overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GaugeProfile {
    initial_units: u64,
    clear_units: u64,
    hit_delta: i64,
    miss_delta: i64,
    fail_on_empty: bool,
    grades: Vec<GradeDelta>,
}

impl GaugeProfile {
    /// Validates bounded levels and unique grades; all signed deltas are valid.
    pub fn new(
        initial_units: u64,
        clear_units: u64,
        hit_delta: i64,
        miss_delta: i64,
        fail_on_empty: bool,
        mut grades: Vec<GradeDelta>,
    ) -> Result<Self, GaugeError> {
        if initial_units > MAX_GAUGE_UNITS || clear_units > MAX_GAUGE_UNITS {
            return Err(GaugeError::InvalidConfiguration(
                "gauge level exceeds 100 percent",
            ));
        }
        if grades.len() > MAX_GAUGE_GRADES {
            return Err(GaugeError::GradeCapacity);
        }
        grades.sort_unstable_by_key(|entry| entry.grade);
        for pair in grades.windows(2) {
            if pair[0].grade == pair[1].grade {
                return Err(GaugeError::DuplicateGrade {
                    grade: pair[0].grade,
                });
            }
        }
        Ok(Self {
            initial_units,
            clear_units,
            hit_delta,
            miss_delta,
            fail_on_empty,
            grades,
        })
    }

    fn hit_delta(&self, grade: JudgeGrade) -> i64 {
        self.grades
            .binary_search_by_key(&grade, |entry| entry.grade)
            .map_or(self.hit_delta, |index| self.grades[index].delta)
    }
}

impl Default for GaugeProfile {
    fn default() -> Self {
        Self {
            initial_units: 20 * GAUGE_UNITS_PER_PERCENT,
            clear_units: 80 * GAUGE_UNITS_PER_PERCENT,
            hit_delta: GAUGE_UNITS_PER_PERCENT as i64,
            miss_delta: -6 * GAUGE_UNITS_PER_PERCENT as i64,
            fail_on_empty: false,
            grades: Vec::new(),
        }
    }
}

/// Application gauge state; it neither judges inputs nor controls audio/cleanup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsGauge {
    profile: GaugeProfile,
    snapshot: GaugeSnapshot,
}

impl BmsGauge {
    pub fn new(profile: GaugeProfile) -> Self {
        let snapshot = GaugeSnapshot {
            level_units: profile.initial_units,
            failure: (profile.fail_on_empty && profile.initial_units == 0)
                .then_some(GaugeFailure::Depleted),
        };
        Self { profile, snapshot }
    }

    pub fn snapshot(&self) -> &GaugeSnapshot {
        &self.snapshot
    }

    pub fn profile(&self) -> &GaugeProfile {
        &self.profile
    }

    /// Readiness under this policy, without asserting song/output completion.
    pub fn can_clear(&self) -> bool {
        self.snapshot.failure.is_none() && self.snapshot.level_units >= self.profile.clear_units
    }

    /// Applies each actual report once: normal stages, then ordered hazards.
    /// All hazard values are validated before any state change, even after failure.
    pub fn observe(
        &mut self,
        events: &[JudgeEvent],
        hazards: &[HazardEvent],
    ) -> Result<(), GaugeError> {
        for hazard in hazards {
            if !(1..=1295).contains(&hazard.value) {
                return Err(GaugeError::InvalidDamage {
                    value: hazard.value,
                });
            }
        }
        let mut next = self.snapshot;
        for event in events {
            if next.failure.is_some() {
                break;
            }
            let delta = match event.outcome {
                JudgeOutcome::Hit { grade, .. } => self.profile.hit_delta(grade),
                JudgeOutcome::Miss { .. } => self.profile.miss_delta,
            };
            self.apply_delta(&mut next, i128::from(delta));
        }
        for hazard in hazards {
            if next.failure.is_some() {
                break;
            }
            if hazard.outcome == HazardOutcome::Avoided {
                continue;
            }
            if hazard.value == 1295 {
                next.level_units = 0;
                next.failure = Some(GaugeFailure::InstantDeath);
            } else {
                self.apply_delta(&mut next, -(i128::from(hazard.value) * 500_000));
            }
        }
        self.snapshot = next;
        Ok(())
    }

    fn apply_delta(&self, snapshot: &mut GaugeSnapshot, delta: i128) {
        snapshot.level_units =
            (i128::from(snapshot.level_units) + delta).clamp(0, i128::from(MAX_GAUGE_UNITS)) as u64;
        if self.profile.fail_on_empty && snapshot.level_units == 0 {
            snapshot.failure = Some(GaugeFailure::Depleted);
        }
    }
}

impl Default for BmsGauge {
    fn default() -> Self {
        Self::new(GaugeProfile::default())
    }
}
