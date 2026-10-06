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
/// Resolved level-dependent judgment policy. Zero fields retain legacy behavior.
/// Negative judgment deltas scale by 3/5 strictly below `damage_reduction_below`;
/// mines retain their explicit raw damage and do not use that judgment reduction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GaugeDynamics {
    pub minimum_alive: u64,
    pub failure_below: u64,
    pub damage_reduction_below: u64,
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
    AllocationFailed,
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
    dynamics: GaugeDynamics,
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
            dynamics: GaugeDynamics::default(),
        })
    }

    pub const fn initial_units(&self) -> u64 {
        self.initial_units
    }
    pub const fn clear_units(&self) -> u64 {
        self.clear_units
    }
    pub const fn default_hit_delta(&self) -> i64 {
        self.hit_delta
    }
    pub const fn miss_delta(&self) -> i64 {
        self.miss_delta
    }
    pub const fn fail_on_empty(&self) -> bool {
        self.fail_on_empty
    }
    pub fn grades(&self) -> &[GradeDelta] {
        &self.grades
    }
    /// Fallible complete policy copy for replay/result association boundaries.
    pub fn try_copy(&self) -> Result<Self, GaugeError> {
        let mut grades = Vec::new();
        grades
            .try_reserve_exact(self.grades.len())
            .map_err(|_| GaugeError::AllocationFailed)?;
        grades.extend_from_slice(&self.grades);
        Ok(Self {
            initial_units: self.initial_units,
            clear_units: self.clear_units,
            hit_delta: self.hit_delta,
            miss_delta: self.miss_delta,
            fail_on_empty: self.fail_on_empty,
            grades,
            dynamics: self.dynamics,
        })
    }
    pub const fn dynamics(&self) -> GaugeDynamics {
        self.dynamics
    }
    pub fn with_dynamics(mut self, dynamics: GaugeDynamics) -> Result<Self, GaugeError> {
        if [
            dynamics.minimum_alive,
            dynamics.failure_below,
            dynamics.damage_reduction_below,
        ]
        .iter()
        .any(|value| *value > MAX_GAUGE_UNITS)
            || (dynamics.failure_below != 0 && !self.fail_on_empty)
            || (self.initial_units < dynamics.minimum_alive
                && !(self.initial_units == 0 && self.fail_on_empty))
        {
            return Err(GaugeError::InvalidConfiguration(
                "inconsistent gauge dynamics",
            ));
        }
        self.dynamics = dynamics;
        Ok(self)
    }
    /// Resolves adapter rules with an explicit fallback BMS judgment and opaque
    /// grade overrides. No ordering or class is inferred from grade numbers.
    pub fn from_bms_rules(
        rules: beatkernel_bms::BmsGaugeRules,
        fallback: beatkernel_bms::BmsJudgment,
        mapped: &[(JudgeGrade, beatkernel_bms::BmsJudgment)],
    ) -> Result<Self, GaugeError> {
        if mapped.len() > MAX_GAUGE_GRADES {
            return Err(GaugeError::GradeCapacity);
        }
        let mut grades = Vec::new();
        grades
            .try_reserve_exact(mapped.len())
            .map_err(|_| GaugeError::GradeCapacity)?;
        grades.extend(mapped.iter().map(|(grade, judgment)| GradeDelta {
            grade: *grade,
            delta: rules.delta(*judgment),
        }));
        Self::new(
            rules.initial_level() as u64,
            rules.clear_level() as u64,
            rules.delta(fallback),
            rules.delta(beatkernel_bms::BmsJudgment::Poor),
            rules.death_level() != 0,
            grades,
        )?
        .with_dynamics(GaugeDynamics {
            minimum_alive: rules.minimum_level() as u64,
            failure_below: rules.death_level() as u64,
            damage_reduction_below: rules.guts_below() as u64,
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
            dynamics: GaugeDynamics::default(),
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
        let failed = (profile.fail_on_empty && profile.initial_units == 0)
            || profile.initial_units < profile.dynamics.failure_below;
        let snapshot = GaugeSnapshot {
            level_units: if failed { 0 } else { profile.initial_units },
            failure: failed.then_some(GaugeFailure::Depleted),
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
            let delta =
                if delta < 0 && next.level_units < self.profile.dynamics.damage_reduction_below {
                    i128::from(delta) * 3 / 5
                } else {
                    i128::from(delta)
                };
            self.apply_delta(&mut next, delta);
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
        snapshot.level_units = (i128::from(snapshot.level_units) + delta).clamp(
            i128::from(self.profile.dynamics.minimum_alive),
            i128::from(MAX_GAUGE_UNITS),
        ) as u64;
        if (self.profile.fail_on_empty && snapshot.level_units == 0)
            || snapshot.level_units < self.profile.dynamics.failure_below
        {
            snapshot.level_units = 0;
            snapshot.failure = Some(GaugeFailure::Depleted);
        }
    }
}

#[cfg(test)]
#[path = "gauge_dynamics_fixtures.rs"]
mod dynamics_fixtures;

impl Default for BmsGauge {
    fn default() -> Self {
        Self::new(GaugeProfile::default())
    }
}
