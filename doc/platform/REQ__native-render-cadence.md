# Direct native render-start cadence

`audio::cadence::{RenderCadence, RenderCadenceError}` describes a drained native
renderer's direct software scheduling observations. The common capture retains
the first 4096 successful-render timestamp/frame tuples in fixed atomic storage.
Each slot is written once by the serialized render owner. Summary allocation,
sorting and printing occur after worker join or callback unregister/drain, never
on an audio callback. Actual frame deltas support variable blocks and skipped
frames without assuming a polling interval.

Each pair subtracts `floor(delta_start_frames * 1_000_000_000 / applied_rate)`
from the actual host timestamp interval, using checked wide arithmetic. Expected
time flooring adds less than one nanosecond of quantization per pair. Retained
prefix signed extrema and nearest-rank absolute p50/p95/p99/max have their own
scope; total successful captures and unretained captures are separate. The first
point establishes a baseline only. Empty history is absent, not zero jitter.
Regressed host time, empty/nonforward/overlapping frame blocks, arithmetic failure
or summary allocation failure returns a typed error.

`AudioOutputStream::render_cadence()` is an optional owner-side capability with
default `Ok(None)` for implementations that do not provide it. WASAPI supplies it
after worker join. ALSA keeps the `AlsaRenderCadence` and `AlsaCadenceError` public
names as aliases, preserving its existing owner API. CoreAudio provides an
inherent `render_cadence()` after successful unregister/drain. An undrained or
unsupported renderer remains unavailable. This adds no OS dependency to core.

## Measurement boundaries

- ALSA samples CLOCK_MONOTONIC immediately before actual Mixer rendering, recording
  only successful renders. Startup buffer-fill bursts are included. Clock read
  failure uses the backend's existing terminal native-clock error behavior.
- WASAPI samples the stream's shared normalized QPC clock immediately before actual
  Mixer rendering during Running. Ready/prefill rendering is excluded. The
  successful RenderReport supplies block identity; GetPosition/QPC presentation
  samples and owner polling are not substituted.
- CoreAudio samples the shared normalized mach clock immediately before the actual
  Mixer render, after buffer validation. This is pre-Mixer software timing, not the
  native IOProc entry or supplied output presentation timestamp. Only successful
  rendering records a point. The existing callback serialization guards the sole
  writer; no additional callback lock or allocation is introduced.

QPC and mach render diagnostics use `sample_realtime()`, which calls the native
time primitive and performs checked conversion without error boxing or formatting.
General control-thread `sample()` retains its rich errors. This avoids introducing
error-path allocations into native rendering.
Pure clock-source fixtures additionally cover raw ticks, distinct domains,
quantized endpoint subtraction, pre-origin signed time and overflow returning
absent data. They supply no native sampler call or failure-path allocation result;
execution and callback allocation audit remain deferred.

On WASAPI/CoreAudio/ASIO diagnostic clock failure, successful audio rendering is not
replaced with silence or a guessed timestamp. Capture is permanently marked
unavailable and the drained summary returns `TimingUnavailable`. Partial prefix
history does not silently imply complete clock observations. Failed renders add
no successful point. Native backend counters retain their own scopes and cannot
be inferred from these timing deviations.

CoreAudio transfers its preallocated capture to the owner only after callback
disable, successful native stop/unregister and active callback drain. Context,
Mixer and asset cleanup retains its existing order; sorting is deferred until
the caller requests the summary. Failed cleanup leaves context owned and cadence
unavailable for later retry. Native examples print summaries after their stop
attempts, including failure paths with explicit unavailable data.

This instrumentation measures software render scheduling relative to output frame
rate. It does not prove callback-arrival jitter, native buffer admission,
presentation clock accuracy, acoustic timing, underrun causation or physical
latency. ASIO opt-in capture also uses actual pre-render QPC timestamps, excluding
owner-side B priming. `prepare_with_clock` accepts an explicit shared QpcClock;
clock-free `prepare` retains its existing behavior with absent cadence capability.
ASIO returns cadence only after terminal close detaches/drains callbacks, including
retained history after native errors. Live/recorded BMS hosts opt in and print
summaries after stop. Driver system time is never a QPC substitute. Five
portable fixtures are authored for actual frame gaps, prefix exhaustion,
chronology errors, missing clock data and full timestamp spans. Compilation
does not execute them. Device measurements, RT performance audit, independent
review and QA remain deferred under the user's sequencing instruction.
