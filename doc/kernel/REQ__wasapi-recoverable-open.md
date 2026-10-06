# Recover mixer ownership from failed WASAPI opening

WasapiBackend exposes open_recoverable with the existing request/mixer/QPC/options
parameters and MixerOpenFailure<AudioPlatformError>. Existing open delegates and
returns the original backend error, intentionally discarding recovered ownership
to preserve its signature. Preflight refusal returns the original mixer before
native acquisition. Capture frame basis before priming as before.

ALSA and WASAPI share a private statically injected generic worker-launch owner
under platform audio. Its cold slot retains the unique mixer through spawn
refusal; the worker releases its lock/reference before native setup. Adapt errors
at native boundaries, preserving existing Linux IO errors and WASAPI WorkerFailure
classification. Share normal-join recovery with backend-specific extraction of
the existing worker result. No extra real-time lock/allocation/dynamic dispatch.

WASAPI worker setup borrows a control-owned Option<Mixer> until all initial
resources are ready. If prefill fails after transfer, move the same mixer back
before worker destruction. Setup failure, control-event duplication failure or
closed startup receiver must return available ownership after resources retire.
Normal worker return retains its mixer; panic yields unavailable ownership.
COM, MMCSS, native events and partial clients remain worker-thread owned and
are released before failure completion is returned. No fake ready stream,
implicit fallback, empty replacement mixer or silent cursor rewind is allowed.

Retain applied settings, buffer sizing, startup evidence and render timing on
success. Pure preflight fixtures and shared launch/join memory cases prepare
verification without native device/QPC calls. Migrate the existing ALSA refusing
spawner test declaration only as needed for the shared generic contract; retain
all original assertions. Windows-only constructor/resource lifecycle fixtures
remain uncompiled unless a permitted Windows target check is available.

This does not implement automatic application handoff or device rollback, prove
native buffer delivery, or add CoreAudio/ASIO recoverable opening. Panic can lose
the mixer; failure recovery retains current software state, not guaranteed
pre-priming cursor state. Assertions/runtime/device/formal review/QA remain
deferred. Scoped Rustfmt and the four existing sequential compile-only checks
follow both writer terminal stops. Full BMS player Goal remains incomplete.
