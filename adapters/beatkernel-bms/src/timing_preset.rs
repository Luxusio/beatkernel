use beatkernel::{
    interaction::{ProfiledHoldEvaluator, ProfiledInstantEvaluator},
    judge::{JudgeError, JudgeGrade, JudgeProfile, JudgeWindow, Rule},
    time::Duration,
};

use crate::{BmsChart, BmsInputMode, BmsJudgeDifficulty, BmsJudgment};
use std::collections::{BTreeMap, BTreeSet};

/// Explicit numerical-table version; this is not an interaction dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsTimingPreset {
    /// SEVENKEYS hit windows pinned to beatoraja commit 8320241d, rates 100.
    BeatorajaSevenKeys8320241dV1,
}

/// Object/stage classification selecting one of the four numerical profiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsTimingStage {
    /// Key press, including a long-note head.
    KeyHead,
    /// Scratch press, including a long-note head.
    ScratchHead,
    /// Key long-note release.
    KeyTail,
    /// Scratch long-note release.
    ScratchTail,
}

/// Explicitly classified inclusive hit window, with grade IDs 1 through 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmsClassifiedTimingWindow {
    /// BMS hit class corresponding to the core grade.
    pub judgment: BmsJudgment,
    /// Validated nested bounds in core input-minus-target convention.
    pub window: JudgeWindow,
}

/// Failure resolving an explicitly selected numerical preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsTimingPresetError {
    /// This integer dialect does not interpret fractional DEFEXRANK.
    FractionalDefExRank,
    /// Zero metadata is valid but has no supported preset fallback.
    ZeroDefExRank,
    /// An intermediate or final nanosecond extent exceeded supported bounds.
    Overflow,
}
impl std::fmt::Display for BmsTimingPresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BMS timing preset: {self:?}")
    }
}
impl std::error::Error for BmsTimingPresetError {}

/// Immutable applied numerical profiles retaining their source declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmsTimingProfiles {
    preset: BmsTimingPreset,
    difficulty: BmsJudgeDifficulty,
    percentage: i128,
    profiles: [[BmsClassifiedTimingWindow; 4]; 4],
}

impl BmsTimingPreset {
    /// Stable numerical-table identity; interaction semantics are separate.
    pub const fn id(self) -> &'static str {
        match self {
            Self::BeatorajaSevenKeys8320241dV1 => {
                "beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1"
            }
        }
    }

    /// Resolves explicit difficulty without choosing a missing-header default.
    /// Original microsecond extents are scaled before conversion to nanoseconds.
    pub fn resolve(
        self,
        difficulty: BmsJudgeDifficulty,
    ) -> Result<BmsTimingProfiles, BmsTimingPresetError> {
        let percentage = match difficulty {
            BmsJudgeDifficulty::Rank(rank) => [25, 50, 75, 100, 125][usize::from(rank.code())],
            BmsJudgeDifficulty::DefExRank(value) => {
                if value.denominator() != 1 {
                    return Err(BmsTimingPresetError::FractionalDefExRank);
                }
                if value.numerator() == 0 {
                    return Err(BmsTimingPresetError::ZeroDefExRank);
                }
                value
                    .numerator()
                    .checked_mul(75)
                    .ok_or(BmsTimingPresetError::Overflow)?
                    / 100
            }
        };
        // Facts from the pinned table, rewritten in core early/late convention.
        // Rows are key head, scratch head, key tail, scratch tail; units are us.
        let bounds = [
            [
                [20_000, 20_000],
                [60_000, 60_000],
                [150_000, 150_000],
                [220_000, 280_000],
            ],
            [
                [30_000, 30_000],
                [70_000, 70_000],
                [160_000, 160_000],
                [230_000, 290_000],
            ],
            [
                [120_000, 120_000],
                [160_000, 160_000],
                [200_000, 200_000],
                [220_000, 280_000],
            ],
            [
                [130_000, 130_000],
                [170_000, 170_000],
                [210_000, 210_000],
                [230_000, 290_000],
            ],
        ];
        let classes = [
            BmsJudgment::PGreat,
            BmsJudgment::Great,
            BmsJudgment::Good,
            BmsJudgment::Bad,
        ];
        let empty = BmsClassifiedTimingWindow {
            judgment: BmsJudgment::PGreat,
            window: JudgeWindow {
                grade: JudgeGrade(1),
                early: Duration::ZERO,
                late: Duration::ZERO,
            },
        };
        let mut profiles = [[empty; 4]; 4];
        for (row, base) in profiles.iter_mut().zip(bounds) {
            for (index, (entry, [early, late])) in row.iter_mut().zip(base).enumerate() {
                *entry = BmsClassifiedTimingWindow {
                    judgment: classes[index],
                    window: JudgeWindow {
                        grade: JudgeGrade(index as u32 + 1),
                        early: extent(early, percentage)?,
                        late: extent(late, percentage)?,
                    },
                };
            }
        }
        Ok(BmsTimingProfiles {
            preset: self,
            difficulty,
            percentage,
            profiles,
        })
    }
}

fn extent(micros: i128, percentage: i128) -> Result<Duration, BmsTimingPresetError> {
    let scaled = micros
        .checked_mul(percentage)
        .ok_or(BmsTimingPresetError::Overflow)?
        / 100;
    let nanos = scaled
        .checked_mul(1000)
        .ok_or(BmsTimingPresetError::Overflow)?;
    Ok(Duration::from_nanos(
        i64::try_from(nanos).map_err(|_| BmsTimingPresetError::Overflow)?,
    ))
}

impl BmsTimingProfiles {
    /// Numerical-table identity used to resolve these immutable windows.
    pub const fn preset(&self) -> BmsTimingPreset {
        self.preset
    }
    /// Exact selected metadata declaration, retaining RANK versus DEFEXRANK.
    pub const fn difficulty(&self) -> BmsJudgeDifficulty {
        self.difficulty
    }
    /// Effective integer percentage after the declaration conversion.
    pub const fn effective_percentage(&self) -> i128 {
        self.percentage
    }
    /// Borrows four nested inclusive windows for the selected stage.
    pub fn windows(&self, stage: BmsTimingStage) -> &[BmsClassifiedTimingWindow; 4] {
        &self.profiles[match stage {
            BmsTimingStage::KeyHead => 0,
            BmsTimingStage::ScratchHead => 1,
            BmsTimingStage::KeyTail => 2,
            BmsTimingStage::ScratchTail => 3,
        }]
    }

    /// Builds zero-offset selected stage windows; calibration belongs to the engine.
    pub fn judge_profile(&self, stage: BmsTimingStage) -> Result<JudgeProfile, JudgeError> {
        JudgeProfile::new(
            self.windows(stage)
                .iter()
                .map(|entry| entry.window)
                .collect(),
            Duration::ZERO,
        )
    }

    /// Builds the head-only indexed routing envelope with one caller calibration.
    /// Actual interactions still use their narrower selected head/tail profiles.
    pub fn head_envelope(&self, offset: Duration) -> Result<JudgeProfile, JudgeError> {
        let windows = self
            .windows(BmsTimingStage::KeyHead)
            .iter()
            .zip(self.windows(BmsTimingStage::ScratchHead))
            .map(|(key, scratch)| JudgeWindow {
                grade: key.window.grade,
                early: key.window.early.max(scratch.window.early),
                late: key.window.late.max(scratch.window.late),
            })
            .collect();
        JudgeProfile::new(windows, offset)
    }
}

impl BmsChart {
    /// Registers selected numerical head/tail windows with BeatKernel Hold v1.
    /// This opt-in path preserves source lanes and indexed button/contact routing.
    pub fn rules_with_timing_profiles(
        &self,
        mode: BmsInputMode,
        profiles: &BmsTimingProfiles,
    ) -> Result<Vec<Rule>, JudgeError> {
        let key_head = profiles.judge_profile(BmsTimingStage::KeyHead)?;
        let scratch_head = profiles.judge_profile(BmsTimingStage::ScratchHead)?;
        let key_tail = profiles.judge_profile(BmsTimingStage::KeyTail)?;
        let scratch_tail = profiles.judge_profile(BmsTimingStage::ScratchTail)?;
        let lanes: BTreeMap<_, _> = self
            .notes
            .iter()
            .map(|note| (note.object, note.lane))
            .collect();
        let mut registered = BTreeSet::new();
        let mut rules = Vec::new();
        let contacts = mode == BmsInputMode::ButtonOrContact;
        for object in &self.source.objects {
            let Some(lane) = lanes.get(&object.id) else {
                continue;
            };
            if !registered.insert(object.interaction) {
                continue;
            }
            let (head, tail) = if lane.is_scratch() {
                (&scratch_head, &scratch_tail)
            } else {
                (&key_head, &key_tail)
            };
            rules.push(Rule {
                interaction: object.interaction,
                control: lane.control(),
                evaluator: if object.end.is_some() {
                    Box::new(ProfiledHoldEvaluator::new(
                        head.clone(),
                        tail.clone(),
                        contacts,
                    )?)
                } else {
                    Box::new(ProfiledInstantEvaluator::new(head.clone(), contacts)?)
                },
            });
        }
        Ok(rules)
    }
}
