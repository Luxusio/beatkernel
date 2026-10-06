//! Pure LR2-compatible gauge rules: #TOTAL resolution, per-judgment deltas and
//! clear/failure thresholds in exact fixed point. No IO, allocation or floats.
//! Mapping opaque core grades to BMS judgments belongs to the application.
use crate::{BmsChart, rational};

/// Fixed-point units in one gauge percentage point, matching runtime gauges.
pub const GAUGE_UNITS_PER_PERCENT: i64 = 1_000_000;
/// The full gauge level, representing 100 percent.
pub const MAX_GAUGE_LEVEL: i64 = 100 * GAUGE_UNITS_PER_PERCENT;
/// Fixed-point units in one #TOTAL point; finer source digits truncate.
pub const TOTAL_UNITS: u64 = 1_000_000;

/// BMS judgment classes in table order; EmptyPoor judges no note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BmsJudgment {
    /// Perfect great.
    PGreat,
    /// Great.
    Great,
    /// Good.
    Good,
    /// Bad.
    Bad,
    /// Missed note (POOR).
    Poor,
    /// Press that judged no note (empty/excess POOR).
    EmptyPoor,
}

impl BmsJudgment {
    /// Every judgment in delta-table order.
    pub const ALL: [Self; 6] = [
        Self::PGreat,
        Self::Great,
        Self::Good,
        Self::Bad,
        Self::Poor,
        Self::EmptyPoor,
    ];
    const fn index(self) -> usize {
        self as usize
    }
}

/// LR2 gauge variants; Hazard follows the beatoraja/LR2oraja LR2 table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BmsGaugeKind {
    /// TOTAL-scaled recovery, 60% clear border.
    AssistEasy,
    /// TOTAL-scaled recovery with reduced damage, 80% clear border.
    Easy,
    /// Standard TOTAL-scaled groove gauge, 80% clear border.
    Groove,
    /// Survival gauge with TOTAL/note-count damage scaling and low-level guts.
    Hard,
    /// Survival gauge with doubled hard damage and no guts.
    ExHard,
    /// Survival gauge where any BAD or POOR fails.
    Hazard,
}

impl BmsGaugeKind {
    /// Every variant, easiest first.
    pub const ALL: [Self; 6] = [
        Self::AssistEasy,
        Self::Easy,
        Self::Groove,
        Self::Hard,
        Self::ExHard,
        Self::Hazard,
    ];
    /// Survival gauges start full and fail when the level falls below 2%.
    pub const fn is_survival(self) -> bool {
        matches!(self, Self::Hard | Self::ExHard | Self::Hazard)
    }
}

/// Positive #TOTAL value in `TOTAL_UNITS` fixed point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BmsTotal(u64);

impl BmsTotal {
    /// Accepts any positive fixed-point value.
    pub const fn from_units(units: u64) -> Option<Self> {
        if units == 0 { None } else { Some(Self(units)) }
    }
    /// Exact fixed-point value.
    pub const fn units(self) -> u64 {
        self.0
    }
    /// Parses a plain positive decimal (optional `+`, at most 18 digits).
    /// Digits finer than one millionth truncate; a zero result is invalid.
    pub fn parse(text: &str) -> Option<Self> {
        let ratio = rational::decimal(text.trim(), 0).ok()?;
        let units = ratio.n.checked_mul(i128::from(TOTAL_UNITS))? / ratio.d;
        Self::from_units(u64::try_from(units).ok()?)
    }
    /// LR2 default for an absent/invalid header:
    /// `160 + (n + clamp(n - 400, 0, 200)) * 0.16`, saturating at `u64::MAX`.
    pub fn lr2_default(notes: u64) -> Self {
        let notes = u128::from(notes);
        let extra = notes.saturating_sub(400).min(200);
        let units = 160 * u128::from(TOTAL_UNITS) + (notes + extra) * 160_000;
        Self(u64::try_from(units).unwrap_or(u64::MAX))
    }
}

/// Why a resolved TOTAL has its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotalSource {
    /// A valid positive #TOTAL header.
    Declared,
    /// No #TOTAL header; the LR2 default applies.
    Absent,
    /// Malformed, zero or negative #TOTAL; the LR2 default applies.
    Invalid,
}

/// Effective TOTAL plus its explicit provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedTotal {
    /// Value used by gauge rules.
    pub total: BmsTotal,
    /// Declared value or the reason the default was used.
    pub source: TotalSource,
}

impl ResolvedTotal {
    /// Resolves optional header text against the judged stage count.
    pub fn resolve(declared: Option<&str>, notes: u64) -> Self {
        match declared.map(BmsTotal::parse) {
            Some(Some(total)) => Self {
                total,
                source: TotalSource::Declared,
            },
            other => Self {
                total: BmsTotal::lr2_default(notes),
                source: if other.is_none() {
                    TotalSource::Absent
                } else {
                    TotalSource::Invalid
                },
            },
        }
    }
}

impl BmsChart {
    /// Judged stages produced by the core judge: one per note plus one per hold
    /// tail. Mines, BGM and invisible selections are never counted.
    pub fn judged_stage_count(&self) -> u64 {
        self.source
            .objects
            .iter()
            .map(|object| 1 + u64::from(object.end.is_some()))
            .sum()
    }
    /// Effective gauge TOTAL from preserved #TOTAL metadata.
    pub fn gauge_total(&self) -> ResolvedTotal {
        ResolvedTotal::resolve(
            self.metadata.get("TOTAL").map(String::as_str),
            self.judged_stage_count(),
        )
    }
    /// LR2 gauge rules for this chart's TOTAL and judged stage count.
    pub fn lr2_gauge_rules(&self, kind: BmsGaugeKind) -> Result<BmsGaugeRules, BmsGaugeError> {
        BmsGaugeRules::lr2(kind, self.gauge_total().total, self.judged_stage_count())
    }
}

/// Invalid gauge rule setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsGaugeError {
    /// A chart without judged stages has no defined TOTAL distribution.
    NoJudgedNotes,
}

impl std::fmt::Display for BmsGaugeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BMS gauge: {self:?}")
    }
}
impl std::error::Error for BmsGaugeError {}

/// Base per-judgment percentages in hundredths, before TOTAL/damage scaling.
const fn base_table(kind: BmsGaugeKind) -> [i64; 6] {
    match kind {
        BmsGaugeKind::AssistEasy | BmsGaugeKind::Easy => [120, 120, 60, -320, -480, -160],
        BmsGaugeKind::Groove => [100, 100, 50, -400, -600, -200],
        BmsGaugeKind::Hard => [10, 10, 5, -600, -1000, -200],
        BmsGaugeKind::ExHard => [10, 10, 5, -1200, -2000, -200],
        BmsGaugeKind::Hazard => [15, 6, 0, -10_000, -10_000, -1000],
    }
}

/// Larger of the LR2 TOTAL and note-count damage multipliers, as num/den.
fn damage_multiplier(total: BmsTotal, notes: u64) -> (i128, i128) {
    let step = (i128::from(total.units()) / (16 * i128::from(TOTAL_UNITS)) - 5).clamp(1, 10);
    let by_total = (10, step);
    let n = i128::from(notes);
    let by_notes = match n {
        0..=20 => (10, 1),
        21..=29 => (70 - n, 5),
        30..=59 => (135 - n, 15),
        60..=124 => (385 - n, 65),
        125..=249 => (625 - n, 125),
        250..=499 => (1000 - n, 250),
        500..=999 => (1500 - n, 500),
        _ => (1, 1),
    };
    if by_total.0 * by_notes.1 >= by_notes.0 * by_total.1 {
        by_total
    } else {
        by_notes
    }
}

/// Setup-resolved LR2 gauge rules; every delta is final fixed-point units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmsGaugeRules {
    kind: BmsGaugeKind,
    deltas: [i64; 6],
    initial: i64,
    minimum: i64,
    death: i64,
    border: i64,
    guts_below: i64,
}

impl BmsGaugeRules {
    /// Resolves LR2 tables. Groove-family recovery scales by TOTAL/notes, and
    /// Hard/ExHard damage by max(TOTAL, note-count) multipliers. Each product is
    /// exact and truncates toward zero; recovery saturates at a full gauge.
    pub fn lr2(kind: BmsGaugeKind, total: BmsTotal, notes: u64) -> Result<Self, BmsGaugeError> {
        if notes == 0 {
            return Err(BmsGaugeError::NoJudgedNotes);
        }
        let percent = GAUGE_UNITS_PER_PERCENT as i128 / 100;
        let damage = damage_multiplier(total, notes);
        let mut deltas = [0; 6];
        for (delta, base) in deltas.iter_mut().zip(base_table(kind)) {
            let units = i128::from(base) * percent;
            let scaled = match kind {
                BmsGaugeKind::AssistEasy | BmsGaugeKind::Easy | BmsGaugeKind::Groove
                    if units > 0 =>
                {
                    units * i128::from(total.units())
                        / (i128::from(notes) * i128::from(TOTAL_UNITS))
                }
                BmsGaugeKind::Hard | BmsGaugeKind::ExHard if units < 0 => {
                    units * damage.0 / damage.1
                }
                _ => units,
            };
            *delta = scaled.min(i128::from(MAX_GAUGE_LEVEL)) as i64;
        }
        let survival = kind.is_survival();
        let percent = |value: i64| value * GAUGE_UNITS_PER_PERCENT;
        Ok(Self {
            kind,
            deltas,
            initial: percent(if survival { 100 } else { 20 }),
            minimum: percent(if survival { 0 } else { 2 }),
            death: percent(if survival { 2 } else { 0 }),
            border: match kind {
                BmsGaugeKind::AssistEasy => percent(60),
                BmsGaugeKind::Easy | BmsGaugeKind::Groove => percent(80),
                _ => 0,
            },
            guts_below: if kind == BmsGaugeKind::Hard {
                percent(32)
            } else {
                0
            },
        })
    }
    /// Selected variant.
    pub const fn kind(&self) -> BmsGaugeKind {
        self.kind
    }
    /// Resolved signed delta for one judgment before low-level guts.
    pub const fn delta(&self, judgment: BmsJudgment) -> i64 {
        self.deltas[judgment.index()]
    }
    /// Starting level.
    pub const fn initial_level(&self) -> i64 {
        self.initial
    }
    /// Floor applied to every living level.
    pub const fn minimum_level(&self) -> i64 {
        self.minimum
    }
    /// Levels strictly below this fail (zero for groove-family gauges).
    pub const fn death_level(&self) -> i64 {
        self.death
    }
    /// Final clear border; survival gauges clear by staying alive.
    pub const fn clear_level(&self) -> i64 {
        self.border
    }
    /// Damage is multiplied by 3/5 strictly below this level (Hard only).
    pub const fn guts_below(&self) -> i64 {
        self.guts_below
    }
    /// Fresh state at the initial level.
    pub const fn start(self) -> BmsGaugeState {
        BmsGaugeState {
            level: self.initial,
            rules: self,
        }
    }
}

/// Copyable live gauge; zero is the latched failed state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmsGaugeState {
    rules: BmsGaugeRules,
    level: i64,
}

impl BmsGaugeState {
    /// Applies one judgment; a failed gauge stays frozen at zero.
    pub fn apply(&mut self, judgment: BmsJudgment) {
        if self.level <= 0 {
            return;
        }
        let mut delta = self.rules.delta(judgment);
        if delta < 0 && self.level < self.rules.guts_below {
            delta = delta * 3 / 5;
        }
        let next = self
            .level
            .saturating_add(delta)
            .clamp(self.rules.minimum, MAX_GAUGE_LEVEL);
        self.level = if next < self.rules.death { 0 } else { next };
    }
    /// Current fixed-point level.
    pub const fn level(&self) -> i64 {
        self.level
    }
    /// Resolved rules of this state.
    pub const fn rules(&self) -> &BmsGaugeRules {
        &self.rules
    }
    /// Latched failure; only survival gauges can reach it.
    pub const fn failed(&self) -> bool {
        self.level <= 0
    }
    /// Clear predicate at song end; it does not assert the song has ended.
    pub const fn qualified(&self) -> bool {
        self.level > 0 && self.level >= self.rules.border
    }
}
