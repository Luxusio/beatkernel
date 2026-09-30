# Explicit software timing and interval jitter

InputDeliveryTelemetry separately observes canonical event timestamp age at an
explicit fresh receipt point in one caller-selected clock domain. It reads no
clock and converts no timestamp. Both points must use the configured domain,
receipt must not precede the event, and successive receipt points must not
regress. Invalid observations leave all state unchanged. Event times themselves
may arrive out of order across devices; gameplay ordering belongs to Runtime.
Checked i128 subtraction yields u64 nanoseconds, including Timestamp::MIN to
Timestamp::MAX. Equal points record a real zero-age sample.

Retention is bounded to 0..65536 durations with fallible setup allocation. Zero
disables retention while successful observations are still counted, saturating.
Recording reuses the existing private duration ring without allocation after
setup. Summary reports retained-tail nearest-rank p50/p95/p99/max and returns None
without retained observations; it copies/sorts outside callbacks. Clones preserve
independent actual history. No physical or nominal interval is inferred.

Native BMS compositions label their input age separately from Runtime processing:
Windows QPC acquisition receipt to runtime, Linux preserved kernel event time to
runtime, and macOS preserved IOHID event time to runtime. A fresh same-domain
observation follows acquisition; event metadata/provenance stays unchanged.
Eligible selected input is observed before gameplay dispatch. Final summaries
print after cleanup on successful and failing paths, with unavailable history
represented as None. These ages include only the boundary represented by each
backend's timestamp; they do not measure physical press-to-sound latency or prove
hardware timestamp accuracy. Execution and hardware measurements remain deferred.

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

The [Linux input cadence example](../platform/REQ__linux-input-cadence.md) now
observes an explicitly selected real evdev keyboard signal or hidraw report ID
against a caller-supplied nominal period. Actual event points feed IntervalJitter;
fresh same-domain receipt points feed delivery age. It retains finite summaries,
source metadata and native counters, stops on loss/order discontinuity and prints
after acquisition closes. Kernel evdev timing and userspace hidraw receipt timing
are labeled separately. This fills native acquisition-to-cadence source wiring;
native execution and measured results remain outstanding.

[Windows Raw Input cadence](../platform/REQ__windows-input-cadence.md) now selects
an explicit keyboard session device/usage/state with a 4096-entry ring. It observes
actual QPC receipt intervals and separately samples receipt-to-inspector age.
[macOS IOHID cadence](../platform/REQ__macos-input-cadence.md) selects a registry
identity, element cookie, HID usage and exact native scalar value with 65536-entry
rings. It observes original mach event timestamps through the shared host mapping.
Both stop on order/time discontinuities and retain first/last provenance; native
errors/removal stop the segment and final summaries follow cleanup. Windows
receipt intervals and IOHID event intervals are different measurement boundaries.
These compositions add source instrumentation, not executed results.

ALSA now captures [render-worker scheduling cadence](../platform/REQ__alsa-render-cadence.md)
directly at the worker's pre-Mixer clock boundary, retaining actual successful
block frames. After join, a finite prefix summary subtracts frame-derived expected
time from observed intervals. Startup fills remain included, and exhausted prefix
capacity is visible. This is distinct from native input cadence, callback arrival,
presentation-grid accuracy and acoustic latency; real hardware measurements and
ASIO direct scheduling capture remains outstanding. WASAPI and CoreAudio now
reuse the [shared prefix capture](../platform/REQ__native-render-cadence.md), with
actual pre-Mixer QPC/mach points and successful variable-block frame identity.
WASAPI excludes Ready prefill; CoreAudio waits for unregister/drain before owner
summary. A missing diagnostic clock yields a typed unavailable result without a
fabricated timestamp or callback-side summary allocation. These observations
are software render-start cadence, not native callback entry or presentation.
