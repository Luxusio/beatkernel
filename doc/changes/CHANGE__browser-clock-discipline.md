# Browser input transport from actual output clock pairs

The shared StepGameplay application owner reuses the existing native
PresentationDiscipline. BrowserGame opts into its bounded preallocated observer;
existing native/shared callers remain opt-in. Actual output timestamps retain
their paired Window performanceTime coordinate. The Worker validates both
coordinates before admitting evidence, independently of the actual Mixer
completion report.

Correction applies continuously at a successfully accepted gameplay watermark,
after its complete input prefix. The shared owner clears correction eligibility
on input and requires the exact latest accepted watermark. Original keyboard
timestamps, committed judgments, past Transport segments and already prepared
audio commands are preserved. A nominal activation projection is not replaced
by a fabricated exact output relation.

No observations or stale observations skip correction and hold the current
transport rate. Repeated output positions do not refresh evidence; advancing
output with unchanged coarse host time waits for host progress. Numeric, domain,
chronology, rate and phase errors remain explicit and fence the owner, retaining
the committed score. Completion continues to use actual presentation positions,
not an extrapolated or corrected UI cursor.

The initial policy retains up to 64 observations with 100 ms retention spacing,
one-second warmup/update spacing, one-second observation freshness, a ten-second
phase correction horizon, 1,000 ppm rate bound and 250 ms phase error bound.
Clock quality remains Unknown; these policy bounds do not measure acoustic
latency or promise perfect synchronization. Capture/replay, durable results and
browser networking still remain unfinished.

Host workspace all-targets, headless application all-targets, WASM browser
library and WASM browser-audio library checks exited zero after all Rust authors
stopped writing. Existing WASM platform dead-code warnings remain. The 18 real
StepGameplay fixture groups compiled; their assertions were not executed.
JavaScript fixture authorship is separate from compilation. Runtime verification,
generated bindings, JavaScript syntax checks, browser/device output, formal
review, QA and acceptance remain deferred at the user's request. The full player
Goal remains active.
