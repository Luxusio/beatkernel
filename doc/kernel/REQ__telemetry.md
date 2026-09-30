# Explicit software timing and interval jitter

RuntimeTelemetry retains its existing API: a bounded ring of processing durations
with nearest-rank p50/p95/p99/max, saturating runtime counters, and host-reported
input drops/underruns. These observations do not prove physical latency or device
starvation. A zero processing-ring capacity disables duration retention.

IntervalJitter is a separate explicit clock-pair observer. Construction supplies a
positive integer-nanosecond nominal interval, caller clock-domain identity and
baseline ClockPoint, plus finite retention capacity (1..65536). One accepted
observation computes elapsed = point.timestamp - baseline.timestamp and signed
deviation = elapsed - nominal. Equal timestamps are valid and produce negative
nominal deviation. Clock-domain mismatch and timestamp regression fail before any
baseline/ring mutation. The caller must explicitly reset on a clock discontinuity,
seek, domain replacement or nominal-interval change; reset validates the new
nominal before discarding any existing observations.

Arithmetic uses checked i128 differences rather than narrowing a full signed
timestamp span to Duration. A Timestamp::MIN -> Timestamp::MAX interval is
u64::MAX nanoseconds and remains representable; its signed deviation may exceed
i64::MAX. The public observation therefore carries u64 elapsed nanoseconds and
i128 signed deviation. Regressions are rejected, never treated as reverse jitter.
The baseline advances only after all checks succeed. Successful observations are
allocation-free after setup and overwrite the oldest retained deviation when full.

Summary is unknown until at least one pair is observed. It reports retained sample
count, minimum/maximum signed deviation and nearest-rank p50/p95/p99/max absolute
deviation. Both extrema and percentiles describe the retained tail, not expired
history; total successful pair count is separately observable and saturating.
Summary copies and sorts at most the configured capacity outside callbacks, using
fallible reservations and explicit allocation errors. Clone shares no mutable
state and preserves the actual bounded history.

These values measure only the explicitly supplied clock points against the chosen
nominal interval. They do not establish acquisition timestamp quality, calibration,
hardware jitter or input-to-output latency. A host may observe native callbacks
with a labeled monotonic clock, but must document acquisition location/domain and
quality independently. Core reads no OS clock and generates no cadence internally.

The offline runtime_bench example additionally feeds a bounded, generated 1 ms
synthetic clock with a small repeating interval-deviation pattern. It prints this
as synthetic interval jitter, independent of CPU execution timing and its varying
render-buffer sizes. Physical input/output latency, native callback-arrival jitter
and actual native underruns remain explicitly unavailable. Benchmark execution,
formal QA and native tests stay deferred by user instruction (2026-09-30).

Authored fixtures cover signed/absolute nearest-rank behavior, equality, ring
retention, unknown-before-pair state, atomic rejection/reset, full signed timestamp
span, nominal validation and original RuntimeTelemetry compatibility. They are
compile-checked during implementation; fixture execution is deferred.
