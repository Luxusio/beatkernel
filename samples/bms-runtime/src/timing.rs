//! Exact constant-storage statistics of accepted builtin-stage timing deltas.
use beatkernel::judge::{JudgeEvent, JudgeOutcome, JudgeStage};

/// Checked scalar accumulation exceeded its finite representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingError {
    Overflow,
}
impl std::fmt::Display for TimingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("accepted timing summary overflow")
    }
}
impl std::error::Error for TimingError {}

/// All accepted known-stage samples, independent of bounded recent HUD history.
/// Grades are opaque and each hold head/tail contributes separately.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimingSummary {
    count: u64,
    early: u64,
    late: u64,
    exact: u64,
    sum: i128,
    absolute_sum: u128,
    last: Option<i64>,
    min: Option<i64>,
    max: Option<i64>,
}
impl TimingSummary {
    #[cfg(test)]
    pub(crate) fn exhausted_for_fixture() -> Self {
        Self {
            count: u64::MAX,
            exact: u64::MAX,
            ..Self::default()
        }
    }
    /// Number of accepted known-stage timing samples.
    pub const fn count(&self) -> u64 {
        self.count
    }
    /// Samples with a negative delta.
    pub const fn early(&self) -> u64 {
        self.early
    }
    /// Samples with a positive delta.
    pub const fn late(&self) -> u64 {
        self.late
    }
    /// Samples with a zero delta.
    pub const fn exact(&self) -> u64 {
        self.exact
    }
    /// Most recently accepted delta in batch order.
    pub const fn last_ns(&self) -> Option<i64> {
        self.last
    }
    /// Smallest accepted delta.
    pub const fn min_ns(&self) -> Option<i64> {
        self.min
    }
    /// Largest accepted delta.
    pub const fn max_ns(&self) -> Option<i64> {
        self.max
    }
    /// Signed average, truncated toward zero.
    pub fn mean_ns(&self) -> Option<i64> {
        if self.count == 0 {
            None
        } else {
            i64::try_from(self.sum / i128::from(self.count)).ok()
        }
    }
    /// Average absolute delta, truncated in unsigned nanoseconds.
    pub fn mean_absolute_ns(&self) -> Option<u64> {
        if self.count == 0 {
            None
        } else {
            u64::try_from(self.absolute_sum / u128::from(self.count)).ok()
        }
    }
    /// Atomically accumulates the full batch. Misses and custom stages contribute
    /// nothing; no allocations, clocks, history scans or floating point are used.
    pub fn observe(&mut self, events: &[JudgeEvent]) -> Result<(), TimingError> {
        let mut next = *self;
        for event in events {
            if !matches!(
                event.stage,
                JudgeStage::Instant | JudgeStage::HoldHead | JudgeStage::HoldTail
            ) {
                continue;
            }
            let JudgeOutcome::Hit { delta, .. } = event.outcome else {
                continue;
            };
            let delta = delta.as_nanos();
            next.count = next.count.checked_add(1).ok_or(TimingError::Overflow)?;
            let counter = if delta < 0 {
                &mut next.early
            } else if delta > 0 {
                &mut next.late
            } else {
                &mut next.exact
            };
            *counter = counter.checked_add(1).ok_or(TimingError::Overflow)?;
            next.sum = next
                .sum
                .checked_add(i128::from(delta))
                .ok_or(TimingError::Overflow)?;
            next.absolute_sum = next
                .absolute_sum
                .checked_add(u128::from(delta.unsigned_abs()))
                .ok_or(TimingError::Overflow)?;
            next.last = Some(delta);
            next.min = Some(next.min.map_or(delta, |old| old.min(delta)));
            next.max = Some(next.max.map_or(delta, |old| old.max(delta)));
        }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use beatkernel::{
        chart::ObjectId,
        judge::{JudgeGrade, MissReason},
        time::{Duration, Timestamp},
    };
    fn hit(delta: i64, stage: JudgeStage) -> JudgeEvent {
        JudgeEvent {
            object: ObjectId(1),
            stage,
            outcome: JudgeOutcome::Hit {
                grade: JudgeGrade(999),
                delta: Duration::from_nanos(delta),
            },
            at: Timestamp::ZERO,
            input: None,
        }
    }
    #[test]
    fn signs_stages_order_and_partition_are_exact() {
        let mut miss = hit(123, JudgeStage::Instant);
        miss.outcome = JudgeOutcome::Miss {
            reason: MissReason::HeadTimeout,
        };
        let events = [
            hit(-7, JudgeStage::Instant),
            hit(2, JudgeStage::HoldHead),
            hit(0, JudgeStage::HoldTail),
            miss,
            hit(900, JudgeStage::Custom(1)),
            hit(-2, JudgeStage::Instant),
        ];
        let mut all = TimingSummary::default();
        all.observe(&events).unwrap();
        assert_eq!(
            (all.count(), all.early(), all.late(), all.exact()),
            (4, 2, 1, 1)
        );
        assert_eq!(all.mean_ns(), Some(-1));
        assert_eq!(all.mean_absolute_ns(), Some(2));
        assert_eq!(all.last_ns(), Some(-2));
        assert_eq!(all.min_ns(), Some(-7));
        assert_eq!(all.max_ns(), Some(2));
        let mut split = TimingSummary::default();
        for chunk in events.chunks(2) {
            split.observe(chunk).unwrap();
        }
        assert_eq!(split, all);
        let mut reversed = TimingSummary::default();
        reversed
            .observe(&events.into_iter().rev().collect::<Vec<_>>())
            .unwrap();
        assert_eq!(reversed.mean_ns(), all.mean_ns());
        assert_eq!(reversed.last_ns(), Some(-7));
        let empty = TimingSummary::default();
        assert_eq!(empty.mean_ns(), None);
        assert_eq!(empty.mean_absolute_ns(), None);
        assert_eq!(empty.last_ns(), None);
    }
    #[test]
    fn signed_extremes_do_not_overflow_or_round() {
        let mut summary = TimingSummary::default();
        summary
            .observe(&[hit(i64::MIN, JudgeStage::Instant)])
            .unwrap();
        assert_eq!(summary.mean_ns(), Some(i64::MIN));
        assert_eq!(summary.mean_absolute_ns(), Some(1u64 << 63));
        summary
            .observe(&[hit(i64::MAX, JudgeStage::HoldHead)])
            .unwrap();
        assert_eq!(summary.mean_ns(), Some(0));
        assert_eq!(summary.mean_absolute_ns(), Some(i64::MAX as u64));
        assert_eq!(summary.min_ns(), Some(i64::MIN));
        assert_eq!(summary.max_ns(), Some(i64::MAX));
    }
    #[test]
    fn every_checked_overflow_rejects_entire_batch() {
        let event = hit(1, JudgeStage::Instant);
        for original in [
            TimingSummary {
                count: u64::MAX,
                ..TimingSummary::default()
            },
            TimingSummary {
                late: u64::MAX,
                ..TimingSummary::default()
            },
            TimingSummary {
                sum: i128::MAX,
                ..TimingSummary::default()
            },
            TimingSummary {
                absolute_sum: u128::MAX,
                ..TimingSummary::default()
            },
        ] {
            let mut state = original;
            assert_eq!(
                state.observe(&[hit(0, JudgeStage::Custom(1)), event]),
                Err(TimingError::Overflow)
            );
            assert_eq!(state, original);
        }
        let original = TimingSummary {
            sum: i128::MIN,
            ..TimingSummary::default()
        };
        let mut state = original;
        assert!(state.observe(&[hit(-1, JudgeStage::HoldTail)]).is_err());
        assert_eq!(state, original);
        let mut near = TimingSummary {
            count: u64::MAX - 1,
            late: u64::MAX - 1,
            ..TimingSummary::default()
        };
        let original = near;
        assert!(near.observe(&[event, event]).is_err());
        assert_eq!(near, original);
    }
}
