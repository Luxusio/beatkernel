# Recover WASAPI output ownership

## Legacy and complete retained modes

The Mixer API below keeps existing legacy behavior, including remixed/scalar/
finite callers. Retained Source adds complete NativeOutputState open/recovery,
selected explicitly at retained startup. Preserve remix, pending PCM/report/
admitted prefix and history; never flatten complete state through Mixer
convenience recovery or disable old scalar/ASIO/backend-switch behavior.

Use first-unsent stream basis plus the original immutable creation pin and
actual newly created Counter frequency. Metadata grants no playback progress.
Preserve operation plus release/Stop/start cleanup/join/recovery failures.

The complete path supports retained shared and event-driven exclusive startup
through the same owner. Reuse existing exclusive initialization, equal actual
buffer/period and explicit alignment negotiation; never fall back to shared mode.
Exclusive filling uses a whole actual native buffer without GetCurrentPadding.
Both prefill and event fills preserve original pending PCM across whole-buffer
leases, including multiple-buffer suffixes; no new PCM may replace retained data.

Known Exact incompatible suffixes refuse before COM. For unresolved defaults or
authorized alignment/rounding, check actual native buffer compatibility after
GetBufferSize but before history/reconfigure/MMCSS/prefill. Empty or whole-multiple
pending is admissible; a nonmultiple returns the original unmodified complete
owner and retires acquired resources. The Source contract owns the same live
empty-suffix retirement and typed geometry-error rules.

Uncertain native release keeps a complete quarantined owner that cannot reopen,
reconfigure or Retry. The generic cold launch carries complete state; the new
path must not erase join failure through an Option conversion.

These are implementation requirements, not native execution evidence. The
[Source contract](../runtime/REQ__retained-source-output-replacement.md) defines
exact coordinates, sole-worker conditional permission and verification tiers.

## Existing Mixer convenience API

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

The legacy API above does not itself implement application handoff or device
rollback, prove native buffer delivery, or add CoreAudio/ASIO recoverable opening.
Panic can lose the mixer; failure recovery retains current software state, not
guaranteed pre-priming cursor state. The separate retained Source composition now
connects Windows solo/local and the shared portable pumps; its executed software
tests and remaining independent/native gates are tracked in the
[current change record](../changes/2026-10-14-retained-source-output-replacement.md).
Rust typing does not establish native buffer delivery. The full player Goal
remains incomplete.
