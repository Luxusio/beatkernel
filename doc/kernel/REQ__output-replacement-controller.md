# Coordinate paused output replacement through static IO ports

Core CommandProducer supplies an exclusive PauseHold through hold_pause.
Acquisition pins effective pause; duplicate acquisition refuses without changing
the existing hold. While held, ordinary resume requests cannot unpause rendering.
Dropping/releasing the hold leaves pause requested, so the caller explicitly
reissues resume after committing pause/clock ownership. Use the existing shared
queue allocation plus one scalar atomic gate, no callback lock or allocation.
Runtime, RuntimeGroup and SoloRuntime expose the same cold hold operation.

The common OutputReplacement controller uses a business-owned static backend
port with presentation/output/request/error types, native open/start/retire,
StoppedMixerSource recovery, immutable stream epoch/basis, original observation,
render report and optional interval pause evidence. No platform branches/imports
or native clock reads belong in its policy. A Linux-only ALSA bridge implements
the port using actual open_recoverable/start/stop/timing/report APIs and an
immutable epoch wrapper. Legacy initial streams use creation epoch zero; new
wrappers receive the attempt epoch during opening.

Attach one actual output without dropping a rejected second owner. Before IO,
validate phase/epoch compatibility, finite positive observation-wait policy and
checked next epoch. Acquire the pause hold, retire the old output and recover
its unique mixer. Never silently ignore retirement errors or drop an unretired
owner. Keep recovered mixer, pending owner or explicit unavailable state on
failure. Stage existing joint pause/presentation timing against the real mixer.
Consume the next epoch immediately before each backend open attempt, including
failures; no wrap or reuse. Verify returned epoch/basis before native start.

Opening/start failure preserves original error and separate cleanup/recovery
diagnostics and retains available software/native ownership. There is no implicit
fallback or retry. On success retain the new output, candidate timing and pause
hold while polling for genuine accepted observations and a paused render report.
Use tagged pause observation paths (point or original interval). Do not return
ready before a real accepted pair, valid report and still-frozen pause state.
Return output, candidate timing and hold together; old caller clocks/Transport
remain untouched until the caller commits them. Releasing hold still leaves
audio paused until explicit coordinated resume.

Caller-supplied monotonic ClockPoint values bound first-observation polling from
the first poll, with fixed host domain, checked wide elapsed time and exact
deadline refusal. This is not cancellation of a blocked foreign open/start call.
Observation errors, timeout and explicit cancel retire the candidate and retain
its recovered mixer or unretired owner. Explicit retirement retry handles pending
owners without inventing proof. The controller reports its state and last issued
epoch for integration; it does not automatically reopen after cleanup errors.

Author core hold/queued-PCM tests and independent full controller traces through
actual memory Mixer/presentation implementations with failed open/start/retire/
observe, stale epoch/wrong basis, timeout/cancel, ownership/drop probes, subsequent
explicit attempts and held resume requests. Compile the real ALSA adapter without
opening devices. Assertions/runtime/formal review/QA remain deferred; scoped
Rustfmt and four sequential compile-only checks follow both paired writer stops.
Windows/macOS adapters, native/UI pump wiring, blocking-call isolation and
physical/acoustic acceptance remain pending. Full BMS player Goal stays active.
