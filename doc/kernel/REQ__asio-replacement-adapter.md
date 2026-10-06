# Bind real ASIO intervals to common output replacement

The SDK-gated ASIO adapter implements existing OutputReplacementBackend through
actual driver open, recoverable prepare with caller QPC clock, start/stop,
StoppedMixerSource and presentation_observation. Requests preserve the existing
unsafe trusted-driver/optional-HWND lifetime contract; no safe wrapper silently
assumes trust in arbitrary installed driver code. Keep !Send/same-thread affinity.
No SDK/dependency/build/license changes or fake native controls.
Keep the app library's forbid(unsafe_code) unchanged. The native platform
control module provides a sealed TrustedAsioOpener capability, constructed under
an explicit unsafe trusted-driver/HWND-lifetime contract without loading a
driver. Its safe open method performs the existing native unsafe operation.
The safe app request owns this capability and delegates actual open after old
retirement. Window/trust obligations extend to every resulting control/stream;
do not weaken the capability or move unsafe code into business policy.

Actual AsioStream captures/exposes its OutputFrameBasis before probes, renderer
construction and B priming. Immutable wrapper creation epochs follow existing
initial-zero/replacement-attempt rules. Driver-open failure retains the original
mixer; prepare failure maps recovered mixer, actual pending stream and separate
cleanup diagnostic without cloning or discarding native ownership. Use absolute
Mixer report identity with basis.origin, never add captured start offset twice.

Shared portable native observation helpers admit optional original
AsioPresentationObservation through tagged discipline after checking epoch.
Missing initial observations wait; invalid identities, malformed intervals and
terminal/native errors refuse. Preserve original output/rate/report and complete
host before/after bounds when producing LivePauseObservation::Interval; do not
replace them with midpoint or receipt time. The real adapter caches only genuine
accepted evidence and supplies its original interval to the common controller.
Basis-aware tagged ASIO discipline distinguishes the original absolute grid
origin in observation.output_origin from the staged stream-zero output origin.
Validate original observation origin/rate against basis, configured origin
against basis.point_at_stream_frame(0), and block start against captured physical
offset; keep the original output point/report/interval unchanged. Retain basis
identity within epoch and refuse silent changes or mixing legacy/basis-aware
admission. Rebind resets identity. Legacy ASIO APIs retain zero-offset behavior.
Existing native Windows gameplay interval projection delegates to the same helper
without changing old input/pause expectations.

Caller supplies the existing multimedia clock anchor and honest latency error
bounds. No assumption of universal driver timestamp reliability or acoustic
accuracy. Port policy stays OS-independent; native clock/device calls remain in
the gated adapter. No extra render callback allocation, lock or dynamic dispatch.

Author independent portable real-Mixer report/interval admission/provenance tests
and SDK-only error mapping with recovered real PCM/queues, without creating
drivers, registry controls, QPC clocks or callback owners. SDK source/tests and
actual stream getter remain uncompiled by four permitted Linux/WASM checks.
Assertions/runtime/formal review/QA remain deferred; scoped Rustfmt and four
sequential compile-only checks follow both paired writer terminal stops.
Native play-loop/UI replacement composition, blocking-call isolation and
platform/device acceptance remain pending. Full BMS player Goal stays active.
