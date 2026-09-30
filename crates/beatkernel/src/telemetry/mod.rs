//! Bounded software timing observations; no inferred physical latency.

mod input_delivery;
pub use input_delivery::{InputDeliveryError, InputDeliveryObservation, InputDeliveryTelemetry};

/// Nearest-rank percentiles of the currently retained software durations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingSummary {
    /// Number of retained observations.
    pub samples: usize,
    /// Median duration, nanoseconds.
    pub p50_ns: u64,
    /// 95th percentile duration, nanoseconds.
    pub p95_ns: u64,
    /// 99th percentile duration, nanoseconds.
    pub p99_ns: u64,
    /// Largest retained duration, nanoseconds.
    pub max_ns: u64,
}

/// Saturating runtime counters, distinct from callback mixer counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeCounters {
    /// Physical events admitted to binding/judging.
    pub inputs: u64,
    /// Physical events with no binding destination.
    pub unbound: u64,
    /// Rejected input or deadline operations.
    pub rejected: u64,
    /// Judge results emitted, including misses.
    pub judge_results: u64,
    /// Commands published to the audio queue.
    pub audio_commands: u64,
    /// Commands rejected by a full queue.
    pub queue_full: u64,
    /// Commands rejected by a disconnected queue.
    pub queue_disconnected: u64,
    /// Input loss explicitly reported by the acquisition host.
    pub input_drops: u64,
    /// Underruns explicitly reported by the output host.
    pub audio_underruns: u64,
}

/// Fixed-capacity ring; snapshots sort a copy outside the callback.
#[derive(Clone, Debug)]
pub struct RuntimeTelemetry {
    durations: Vec<u64>,
    capacity: usize,
    next: usize,
    counters: RuntimeCounters,
}

impl RuntimeTelemetry {
    /// Allocates the ring. Zero disables duration retention, keeping counters.
    pub fn new(capacity: usize) -> Self {
        Self {
            durations: Vec::with_capacity(capacity),
            capacity,
            next: 0,
            counters: RuntimeCounters::default(),
        }
    }

    /// Records a software processing duration in integer nanoseconds.
    pub fn record_processing_ns(&mut self, nanos: u64) {
        if self.capacity == 0 {
            return;
        }
        if self.durations.len() < self.capacity {
            self.durations.push(nanos);
        } else {
            self.durations[self.next] = nanos;
        }
        self.next = (self.next + 1) % self.capacity;
    }

    /// Returns no percentiles until a duration has actually been observed.
    pub fn processing(&self) -> Option<TimingSummary> {
        if self.durations.is_empty() {
            return None;
        }
        let mut sorted = self.durations.clone();
        sorted.sort_unstable();
        let percentile =
            |p: usize| sorted[(sorted.len().saturating_mul(p).div_ceil(100)).saturating_sub(1)];
        Some(TimingSummary {
            samples: sorted.len(),
            p50_ns: percentile(50),
            p95_ns: percentile(95),
            p99_ns: percentile(99),
            max_ns: sorted[sorted.len() - 1],
        })
    }

    /// Current cumulative counters.
    pub const fn counters(&self) -> RuntimeCounters {
        self.counters
    }

    /// Adds observed acquisition loss; the runtime cannot infer missing events.
    pub fn report_input_drops(&mut self, count: u64) {
        self.counters.input_drops = self.counters.input_drops.saturating_add(count);
    }

    /// Adds observed output underruns; silence is not an underrun measurement.
    pub fn report_audio_underruns(&mut self, count: u64) {
        self.counters.audio_underruns = self.counters.audio_underruns.saturating_add(count);
    }

    pub(crate) fn counters_mut(&mut self) -> &mut RuntimeCounters {
        &mut self.counters
    }
}

use crate::time::{ClockDomainId, ClockPoint, Duration};

/// One accepted interval in the explicit caller clock, with signed deviation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntervalObservation {
    /// Accepted point becoming the next interval's baseline.
    pub at: ClockPoint,
    /// Nonnegative elapsed nanoseconds, including the complete signed time span.
    pub elapsed_ns: u64,
    /// Elapsed minus the nominal interval; early/equal pairs may be negative.
    pub deviation_ns: i128,
}

/// Retained-tail signed extrema and nearest-rank absolute jitter percentiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntervalJitterSummary {
    /// Number of retained interval pairs, bounded by configured capacity.
    pub samples: usize,
    /// Smallest signed elapsed-minus-nominal deviation.
    pub min_deviation_ns: i128,
    /// Largest signed elapsed-minus-nominal deviation.
    pub max_deviation_ns: i128,
    /// Median absolute deviation in nanoseconds.
    pub p50_abs_deviation_ns: u64,
    /// 95th percentile absolute deviation in nanoseconds.
    pub p95_abs_deviation_ns: u64,
    /// 99th percentile absolute deviation in nanoseconds.
    pub p99_abs_deviation_ns: u64,
    /// Largest retained absolute deviation in nanoseconds.
    pub max_abs_deviation_ns: u64,
}

/// Explicit interval configuration, clock, chronology or resource failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntervalJitterError {
    /// Retention capacity is zero or exceeds the finite observation limit.
    InvalidCapacity,
    /// The nominal interval must be strictly positive.
    InvalidNominalInterval,
    /// Observation domain differs from the caller-selected baseline domain.
    ClockDomainMismatch {
        /// Required baseline clock identity.
        expected: ClockDomainId,
        /// Received observation clock identity.
        received: ClockDomainId,
    },
    /// Observation timestamp is earlier than the last accepted baseline.
    TimestampRegression,
    /// Checked wide arithmetic or an output conversion was unrepresentable.
    Overflow,
    /// Setup or summary could not reserve its bounded storage.
    AllocationFailed,
}
impl std::fmt::Display for IntervalJitterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "interval jitter: {self:?}")
    }
}
impl std::error::Error for IntervalJitterError {}

/// Finite-capacity observations of explicit clock intervals, without inference.
///
/// No clock is acquired here. Native/physical timing requires independently
/// documented provenance. Summaries allocate outside callbacks; observing an
/// already-configured ring does not allocate. Clones copy the actual history.
#[derive(Clone, Debug)]
pub struct IntervalJitter {
    nominal: Duration,
    baseline: ClockPoint,
    deviations: Vec<i128>,
    capacity: usize,
    next: usize,
    pairs: u64,
}
impl IntervalJitter {
    /// Maximum finite ring observations retained by this API.
    pub const MAX_CAPACITY: usize = 65_536;

    /// Configures a positive nominal interval and explicit domain/baseline.
    ///
    /// ```
    /// use beatkernel::{telemetry::IntervalJitter, time::*};
    /// let baseline = ClockPoint { domain: ClockDomainId(1), timestamp: Timestamp::ZERO };
    /// let mut jitter = IntervalJitter::new(64, Duration::from_nanos(1000), baseline)?;
    /// assert_eq!(jitter.summary()?, None);
    /// let observed = jitter.observe(ClockPoint { timestamp: Timestamp::from_nanos(990), ..baseline })?;
    /// assert_eq!(observed.deviation_ns, -10);
    /// # Ok::<(), beatkernel::telemetry::IntervalJitterError>(())
    /// ```
    pub fn new(
        capacity: usize,
        nominal: Duration,
        baseline: ClockPoint,
    ) -> Result<Self, IntervalJitterError> {
        if capacity == 0 || capacity > Self::MAX_CAPACITY {
            return Err(IntervalJitterError::InvalidCapacity);
        }
        validate_nominal(nominal)?;
        let mut deviations = Vec::new();
        deviations
            .try_reserve_exact(capacity)
            .map_err(|_| IntervalJitterError::AllocationFailed)?;
        Ok(Self {
            nominal,
            baseline,
            deviations,
            capacity,
            next: 0,
            pairs: 0,
        })
    }

    /// Observes a same-domain nondecreasing point, changing no state on error.
    /// Equality is valid; full i64 timestamp spans use checked wide differences.
    pub fn observe(&mut self, at: ClockPoint) -> Result<IntervalObservation, IntervalJitterError> {
        if at.domain != self.baseline.domain {
            return Err(IntervalJitterError::ClockDomainMismatch {
                expected: self.baseline.domain,
                received: at.domain,
            });
        }
        if at.timestamp < self.baseline.timestamp {
            return Err(IntervalJitterError::TimestampRegression);
        }
        let elapsed = i128::from(at.timestamp.as_nanos())
            .checked_sub(i128::from(self.baseline.timestamp.as_nanos()))
            .ok_or(IntervalJitterError::Overflow)?;
        let elapsed_ns = u64::try_from(elapsed).map_err(|_| IntervalJitterError::Overflow)?;
        let deviation_ns = elapsed
            .checked_sub(i128::from(self.nominal.as_nanos()))
            .ok_or(IntervalJitterError::Overflow)?;
        if self.deviations.len() < self.capacity {
            self.deviations.push(deviation_ns);
        } else {
            self.deviations[self.next] = deviation_ns;
        }
        self.next = (self.next + 1) % self.capacity;
        self.baseline = at;
        self.pairs = self.pairs.saturating_add(1);
        Ok(IntervalObservation {
            at,
            elapsed_ns,
            deviation_ns,
        })
    }

    /// Resets for a caller-declared discontinuity/domain or nominal change.
    /// Invalid nominal intervals preserve every previous observation and baseline.
    pub fn reset(
        &mut self,
        nominal: Duration,
        baseline: ClockPoint,
    ) -> Result<(), IntervalJitterError> {
        validate_nominal(nominal)?;
        self.nominal = nominal;
        self.baseline = baseline;
        self.deviations.clear();
        self.next = 0;
        self.pairs = 0;
        Ok(())
    }

    /// Last accepted point, or the explicit construction/reset baseline.
    pub const fn baseline(&self) -> ClockPoint {
        self.baseline
    }
    /// Current caller-selected nominal interval.
    pub const fn nominal_interval(&self) -> Duration {
        self.nominal
    }
    /// Fixed retained sample bound.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
    /// Saturating successful pair count since the latest explicit reset.
    pub const fn observed_pairs(&self) -> u64 {
        self.pairs
    }

    /// Calculates retained-tail percentiles outside observation/callback paths.
    /// Returns None before the first accepted pair; never invents a zero sample.
    pub fn summary(&self) -> Result<Option<IntervalJitterSummary>, IntervalJitterError> {
        let Some(&first) = self.deviations.first() else {
            return Ok(None);
        };
        let mut min = first;
        let mut max = first;
        let mut absolute = Vec::new();
        absolute
            .try_reserve_exact(self.deviations.len())
            .map_err(|_| IntervalJitterError::AllocationFailed)?;
        for &deviation in &self.deviations {
            min = min.min(deviation);
            max = max.max(deviation);
            let magnitude = deviation
                .checked_abs()
                .ok_or(IntervalJitterError::Overflow)?;
            absolute.push(u64::try_from(magnitude).map_err(|_| IntervalJitterError::Overflow)?);
        }
        absolute.sort_unstable();
        let percentile = |p: usize| absolute[(absolute.len() * p).div_ceil(100) - 1];
        Ok(Some(IntervalJitterSummary {
            samples: absolute.len(),
            min_deviation_ns: min,
            max_deviation_ns: max,
            p50_abs_deviation_ns: percentile(50),
            p95_abs_deviation_ns: percentile(95),
            p99_abs_deviation_ns: percentile(99),
            max_abs_deviation_ns: absolute[absolute.len() - 1],
        }))
    }
}
fn validate_nominal(nominal: Duration) -> Result<(), IntervalJitterError> {
    if nominal.as_nanos() <= 0 {
        Err(IntervalJitterError::InvalidNominalInterval)
    } else {
        Ok(())
    }
}
