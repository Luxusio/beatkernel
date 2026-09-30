//! Immutable render-start prefix capture, summarized only after writer drain.
use beatkernel::{telemetry::IntervalJitterSummary, time::Timestamp};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

const CAPACITY: usize = 4096;

/// Drained native renderer's actual successful render-start observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderCadence {
    /// Successful renders observed, saturating at u64::MAX.
    pub successful_renders: u64,
    /// Captured prefix points, at most 4096; first point has no interval.
    pub retained_points: usize,
    /// Successful renders after the fixed prefix filled; not lost audio events.
    pub unretained_renders: u64,
    /// Retained-prefix interval deviation percentiles; None before two points.
    pub intervals: Option<IntervalJitterSummary>,
}

/// Unavailable timing, invalid chronology or bounded summary resource failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderCadenceError {
    /// At least one successful render lacked a usable direct timing observation.
    TimingUnavailable,
    /// Captured host time regressed.
    TimestampRegression,
    /// Render frame starts did not increase or blocks overlapped/were empty.
    FrameRegression,
    /// Timestamp/frame conversion or invalid rate was unrepresentable.
    Overflow,
    /// Summary could not reserve its bounded off-worker sorting storage.
    AllocationFailed,
}
impl std::fmt::Display for RenderCadenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "render cadence: {self:?}")
    }
}
impl std::error::Error for RenderCadenceError {}

struct Point {
    time: AtomicI64,
    start: AtomicU64,
    frames: AtomicU64,
}
pub(crate) struct Capture {
    points: [Point; CAPACITY],
    count: AtomicU64,
    unavailable: AtomicBool,
}
impl Capture {
    pub(crate) fn new() -> Self {
        Self {
            points: std::array::from_fn(|_| Point {
                time: AtomicI64::new(0),
                start: AtomicU64::new(0),
                frames: AtomicU64::new(0),
            }),
            count: AtomicU64::new(0),
            unavailable: AtomicBool::new(false),
        }
    }
    /// Writer-only diagnostic failure; no replacement timestamp is fabricated.
    #[allow(dead_code)]
    pub(crate) fn mark_unavailable(&self) {
        self.unavailable.store(true, Ordering::Release);
    }
    // Worker-only. No slot is overwritten; summary is called after worker join.
    pub(crate) fn record(&self, at: Timestamp, start: u64, frames: u64) {
        let count = self.count.load(Ordering::Relaxed);
        if count < CAPACITY as u64 {
            let point = &self.points[count as usize];
            point.time.store(at.as_nanos(), Ordering::Relaxed);
            point.start.store(start, Ordering::Relaxed);
            point.frames.store(frames, Ordering::Relaxed);
        }
        self.count.store(count.saturating_add(1), Ordering::Release);
    }
    pub(crate) fn summary(&self, rate: u32) -> Result<RenderCadence, RenderCadenceError> {
        if self.unavailable.load(Ordering::Acquire) {
            return Err(RenderCadenceError::TimingUnavailable);
        }
        if rate == 0 {
            return Err(RenderCadenceError::Overflow);
        }
        let count = self.count.load(Ordering::Acquire);
        let retained = count.min(CAPACITY as u64) as usize;
        let mut magnitudes = Vec::new();
        magnitudes
            .try_reserve_exact(retained.saturating_sub(1))
            .map_err(|_| RenderCadenceError::AllocationFailed)?;
        let mut previous = None;
        let mut min = i128::MAX;
        let mut max = i128::MIN;
        for point in &self.points[..retained] {
            let at = point.time.load(Ordering::Relaxed);
            let start = point.start.load(Ordering::Relaxed);
            let frames = point.frames.load(Ordering::Relaxed);
            if frames == 0 {
                return Err(RenderCadenceError::FrameRegression);
            }
            let end = start
                .checked_add(frames)
                .ok_or(RenderCadenceError::Overflow)?;
            if let Some((before, before_start, before_end)) = previous {
                if at < before {
                    return Err(RenderCadenceError::TimestampRegression);
                }
                if start <= before_start || start < before_end {
                    return Err(RenderCadenceError::FrameRegression);
                }
                let expected = i128::from(start - before_start) * 1_000_000_000 / i128::from(rate);
                let residual = i128::from(at) - i128::from(before) - expected;
                min = min.min(residual);
                max = max.max(residual);
                magnitudes
                    .push(u64::try_from(residual.abs()).map_err(|_| RenderCadenceError::Overflow)?);
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
        Ok(RenderCadence {
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
            Err(RenderCadenceError::TimestampRegression)
        );
        assert_eq!(capture.summary(0), Err(RenderCadenceError::Overflow));
        let overlap = Capture::new();
        overlap.record(Timestamp::ZERO, 0, 2);
        overlap.record(Timestamp::ZERO, 1, 1);
        assert_eq!(
            overlap.summary(1000),
            Err(RenderCadenceError::FrameRegression)
        );
    }
    #[test]
    fn missing_native_timing_never_becomes_zero_jitter() {
        let capture = Capture::new();
        capture.record(Timestamp::ZERO, 0, 1);
        capture.mark_unavailable();
        capture.record(Timestamp::from_nanos(1_000_000), 1, 1);
        assert_eq!(
            capture.summary(1000),
            Err(RenderCadenceError::TimingUnavailable)
        );
    }
    #[test]
    fn full_host_span_and_unrepresentable_frame_residual_use_wide_arithmetic() {
        let capture = Capture::new();
        capture.record(Timestamp::MIN, 0, 1);
        capture.record(Timestamp::MAX, 1, 1);
        let intervals = capture.summary(1_000_000_000).unwrap().intervals.unwrap();
        assert_eq!(intervals.max_abs_deviation_ns, u64::MAX - 1);
        let huge = Capture::new();
        huge.record(Timestamp::ZERO, 0, 1);
        huge.record(Timestamp::ZERO, u64::MAX - 1, 1);
        assert_eq!(huge.summary(1), Err(RenderCadenceError::Overflow));
    }
}
