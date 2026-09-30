//! Bounded software timing observations; no inferred physical latency.

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
