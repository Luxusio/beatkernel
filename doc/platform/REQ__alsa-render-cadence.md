# ALSA render-worker scheduling cadence

`AlsaStream::render_cadence()` supplies direct worker-boundary timing for plan
P11 and §17.4 through the [shared native capture](REQ__native-render-cadence.md).
Its public result/error names remain aliases of the shared portable types.
It returns `Ok(None)` while the worker handle remains unjoined.
After `stop()` joins, it returns a checked summary, including partial history
when the worker failed or panicked. No observed points yields an absent interval
summary, not fabricated zero jitter. This is separate from presentation timing.

Immediately before each actual Mixer render, the worker samples CLOCK_MONOTONIC
in the applied request's domain. Only a successful render records that sampled
timestamp with the actual RenderReport start frame and frame count. This is the
software render-start boundary, before conversion/native submission. Failed
render publishes no successful point; a successful render followed by native
failure remains captured. No owner-polling timestamp is substituted or paired
with independently published counters.

A fixed 4096-point atomic prefix is created during stream setup. One worker
writes each slot once and release-publishes its count. There is no wrapping,
overwriting, formatting, sorting, allocation or lock in recording. The only new
native operation is the explicit monotonic clock read; its failure propagates as
a terminal worker error under the existing Linux clock policy. Summary reads
only after join, so it cannot race with a writer or sample a partially written
slot. Totals saturate, and successful renders beyond the prefix increment the
visible `unretained_renders` count. These are unretained measurements, not lost
audio frames or native event loss. Recording does not change scheduling/routing.

For each retained pair, expected time is
`floor((next.start_frame - previous.start_frame) * 1_000_000_000 / applied_rate)`.
Signed residual is actual timestamp difference minus that expected time.
Checked wide arithmetic handles actual frame gaps rather than assuming one
period elapsed between observations. Integer flooring introduces less than one
nanosecond of quantization in the expected interval. Empty/overlapping/nonforward
blocks, host-time regression, unrepresentable summary magnitude or allocation
failure returns a typed error. Equal host timestamps are valid negative residuals.
The summary reports retained-prefix pair count, signed extrema and nearest-rank
p50/p95/p99/max absolute deviations; it sorts at most 4095 values off the worker.

Startup buffer-fill bursts are included in the prefix. They are real render
worker intervals and must not be interpreted as steady-state device callback
cadence. Applied rate, buffer and period settings accompany the Linux example's
output. The existing `linux_native audio` command prints this summary after
stop/join, alongside actual xrun/suspend/failure counters and last core rendering.
Different exact buffer/period configurations can be measured in separate caller
invocations without implicit rounding or fallback.

This measures render-worker scheduling residuals, not device delivery, callback
arrival, acoustic jitter, underrun causation or physical latency. Snapshot
presentation fields and aggregate counters retain their own independent scopes.
Windows and macOS now use the shared direct render-start capture at their own
documented boundaries; ASIO opt-in QPC capture is also implemented separately
from driver presentation time.
The shared authored arithmetic/chronology/prefix
fixtures compile but remain unexecuted; actual buffer-size measurements, RT audit,
independent review and QA remain deferred by the user's instruction.
