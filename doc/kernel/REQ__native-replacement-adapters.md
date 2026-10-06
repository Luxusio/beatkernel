# Bind WASAPI and CoreAudio to common replacement policy

Windows WASAPI and macOS CoreAudio implement the existing static
OutputReplacementBackend contract with immutable creation-epoch wrappers,
actual frame basis, native lifecycle, original observations and render reports.
Legacy initial streams retain epoch zero; replacement streams receive their
attempt token during opening. Never retag cached old observations.

WASAPI uses the existing recoverable opener and caller-supplied QPC/options,
converts original failure/model state into the common OutputOpenFailure shape,
and delegates start/stop/StoppedMixerSource. Observation admission uses actual
stream snapshot and basis-aware tagged discipline. Extract this small native
admission adapter into an unconditional portable module used by the actual
WASAPI bridge; tests exercise memory snapshots through the same helper. Epoch
refusal precedes metadata interpretation. Ready/unavailable/before-first-sample
states wait without fake evidence; terminal status and malformed/native identity
errors remain explicit rather than triggering fallback.

CoreAudio captures/exposes OutputFrameBasis on the actual stream before native
configuration or callback transfer. Absolute CoreAudio render/presentation
points must not receive that offset twice. Bind its actual Mach mapping,
configuration/callback status and coreaudio_presentation_pair to tagged supplied
pair discipline. Map recoverable opening failures without dropping pending
partial streams or original/cleanup diagnostics; pending wrappers retain actual
epoch and captured basis for explicit retirement retry. Preserve !Send/native
thread affinity and existing callback-context drop safety.

The common controller remains free of platform imports/branches. No new native
calls enter observation policy helpers, and no additional callback allocation,
lock, Mixer clone, SDK/dependency or license change is introduced. Successful
wrappers expose read-only streams for eventual play-loop composition.

Author portable actual WASAPI snapshot admission tests plus target-only failure
mapping tests using real Mixer/error values without constructing fake streams,
QPC/Mach clocks or native callbacks. Windows/macOS modules and target-only tests
remain uncompiled by the four existing Linux/WASM checks. Assertions/runtime/
device/formal review/QA remain deferred; scope formatting and four sequential
compile-only checks follow both writer terminal stops. ASIO now follows
[ASIO replacement adapter](REQ__asio-replacement-adapter.md);
native/UI pump wiring and platform/physical acceptance remain pending. Full
BMS player Goal remains active.
