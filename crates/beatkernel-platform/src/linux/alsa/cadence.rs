//! Immutable prefix capture; the ALSA worker is the only writer.
use beatkernel::{telemetry::IntervalJitterSummary, time::Timestamp};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

const CAPACITY: usize = 4096;

/// Joined worker's actual successful render-start scheduling observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlsaRenderCadence {
    /// Successful renders observed, saturating at u64::MAX.
    pub successful_renders: u64,
    /// Captured prefix points, at most 4096; first point has no interval.
    pub retained_points: usize,
    /// Successful renders after the fixed prefix filled; not lost audio events.
    pub unretained_renders: u64,
    /// Retained-prefix interval deviation percentiles; None before two points.
    pub intervals: Option<IntervalJitterSummary>,
}

/// Invalid captured chronology, arithmetic or bounded summary allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlsaCadenceError {
    /// Captured host time regressed.
    TimestampRegression,
    /// Render frame starts did not increase or blocks overlapped/were empty.
    FrameRegression,
    /// Timestamp/frame conversion or invalid rate was unrepresentable.
    Overflow,
    /// Summary could not reserve its bounded off-worker sorting storage.
    AllocationFailed,
}
impl std::fmt::Display for AlsaCadenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ALSA render cadence: {self:?}")
    }
}
impl std::error::Error for AlsaCadenceError {}

struct Point {
    time: AtomicI64,
    start: AtomicU64,
    frames: AtomicU64,
}
pub(super) struct Capture {
    points: [Point; CAPACITY],
    count: AtomicU64,
}
impl Capture {
    pub(super) fn new() -> Self {
        Self {
            points: std::array::from_fn(|_| Point {
                time: AtomicI64::new(0),
                start: AtomicU64::new(0),
                frames: AtomicU64::new(0),
            }),
            count: AtomicU64::new(0),
        }
    }
    // Worker-only. No slot is overwritten; summary is called after worker join.
    pub(super) fn record(&self, at: Timestamp, start: u64, frames: u64) {
        let count = self.count.load(Ordering::Relaxed);
        if count < CAPACITY as u64 {
            let point = &self.points[count as usize];
            point.time.store(at.as_nanos(), Ordering::Relaxed);
            point.start.store(start, Ordering::Relaxed);
            point.frames.store(frames, Ordering::Relaxed);
        }
        self.count.store(count.saturating_add(1), Ordering::Release);
    }
    pub(super) fn summary(&self, rate: u32) -> Result<AlsaRenderCadence, AlsaCadenceError> {
        if rate == 0 {
            return Err(AlsaCadenceError::Overflow);
        }
        let count = self.count.load(Ordering::Acquire);
        let retained = count.min(CAPACITY as u64) as usize;
        let mut magnitudes = Vec::new();
        magnitudes
            .try_reserve_exact(retained.saturating_sub(1))
            .map_err(|_| AlsaCadenceError::AllocationFailed)?;
        let mut previous = None;
        let mut min = i128::MAX;
        let mut max = i128::MIN;
        for point in &self.points[..retained] {
            let at = point.time.load(Ordering::Relaxed);
            let start = point.start.load(Ordering::Relaxed);
            let frames = point.frames.load(Ordering::Relaxed);
            if frames == 0 {
                return Err(AlsaCadenceError::FrameRegression);
            }
            let end = start
                .checked_add(frames)
                .ok_or(AlsaCadenceError::Overflow)?;
            if let Some((before, before_start, before_end)) = previous {
                if at < before {
                    return Err(AlsaCadenceError::TimestampRegression);
                }
                if start <= before_start || start < before_end {
                    return Err(AlsaCadenceError::FrameRegression);
                }
                let expected = i128::from(start - before_start) * 1_000_000_000 / i128::from(rate);
                let residual = i128::from(at) - i128::from(before) - expected;
                min = min.min(residual);
                max = max.max(residual);
                magnitudes
                    .push(u64::try_from(residual.abs()).map_err(|_| AlsaCadenceError::Overflow)?);
            }
            previous = Some((at, start, end));
        }
        magnitudes.sort_unstable();
        let intervals = if magnitudes.is_empty() {
            None
        } else {
            let percentile = |p: usize| magnitudes[(magnitudes.len() * p).div_ceil(100) - 1];
            Some(IntervalJitterSummary {
                samples: magnitudes.len(),
                min_deviation_ns: min,
                max_deviation_ns: max,
                p50_abs_deviation_ns: percentile(50),
                p95_abs_deviation_ns: percentile(95),
                p99_abs_deviation_ns: percentile(99),
                max_abs_deviation_ns: *magnitudes.last().unwrap(),
            })
        };
        Ok(AlsaRenderCadence {
            successful_renders: count,
            retained_points: retained,
            unretained_renders: count.saturating_sub(retained as u64),
            intervals,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_frame_gaps_and_equal_host_points_have_signed_residuals() {
        let capture = Capture::new();
        assert_eq!(capture.summary(1000).unwrap().intervals, None);
        capture.record(Timestamp::ZERO, 0, 1);
        capture.record(Timestamp::ZERO, 1, 1);
        capture.record(Timestamp::from_nanos(3_000_000), 3, 1);
        let summary = capture.summary(1000).unwrap().intervals.unwrap();
        assert_eq!(summary.samples, 2);
        assert_eq!(summary.min_deviation_ns, -1_000_000);
        assert_eq!(summary.max_deviation_ns, 1_000_000);
        assert_eq!(summary.p99_abs_deviation_ns, 1_000_000);
    }
    #[test]
    fn prefix_exhaustion_is_visible_and_never_replaces_old_points() {
        let capture = Capture::new();
        for index in 0..CAPACITY + 7 {
            capture.record(
                Timestamp::from_nanos(index as i64 * 1_000_000),
                index as u64,
                1,
            );
        }
        let summary = capture.summary(1000).unwrap();
        assert_eq!(summary.retained_points, CAPACITY);
        assert_eq!(summary.unretained_renders, 7);
        assert_eq!(summary.intervals.unwrap().max_abs_deviation_ns, 0);
    }
    #[test]
    fn malformed_chronology_and_zero_rate_remain_errors() {
        let capture = Capture::new();
        capture.record(Timestamp::from_nanos(10), 0, 1);
        capture.record(Timestamp::ZERO, 1, 1);
        assert_eq!(
            capture.summary(1000),
            Err(AlsaCadenceError::TimestampRegression)
        );
        assert_eq!(capture.summary(0), Err(AlsaCadenceError::Overflow));
        let overlap = Capture::new();
        overlap.record(Timestamp::ZERO, 0, 2);
        overlap.record(Timestamp::ZERO, 1, 1);
        assert_eq!(
            overlap.summary(1000),
            Err(AlsaCadenceError::FrameRegression)
        );
    }
}
