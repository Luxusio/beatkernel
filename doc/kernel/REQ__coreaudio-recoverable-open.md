# Preserve failed CoreAudio owners until callback retirement

Core OutputOpenFailure<E, S> retains an original open error, optional recovered
Mixer, optional pending native owner S and optional latest cleanup error E.
Construct recovered and pending states without cloning. Borrow original error,
available mixer, pending owner and cleanup diagnostic; into_parts moves every
field. A statically injected retry_retirement closure operates on the pending
owner. On cleanup refusal retain that same owner and original open error while
updating the cleanup diagnostic. Only successful retirement may publish its
returned optional mixer, clear/drop the owner and cleanup diagnostic. No pending
owner returns an explicit no-op. The core reads no native clocks/devices, uses
no allocation/lock/dynamic dispatch, and documents that retirement evidence is
the adapter's responsibility.

CoreAudio exposes open_recoverable(request, clock, mixer) with the pending-owner
failure shape. Existing open delegates and returns the original CoreAudioError,
discarding optional ownership under its existing Drop safety behavior. Pure
request preflight and all native configuration/storage errors before context
transfer return the original mixer. Stage those native operations while the
caller still owns it. Device-global rate/buffer effects are not rolled back.

After the mixer enters a stable callback Context, IOProc/listener registration
failures invoke the existing stop/unregister/drain path before extracting mixer
ownership. Preserve the original registration error even if cleanup also fails.
Failed cleanup returns the actual partial stream as a pending owner, retaining
callback storage and registrations for retry. Never take its UnsafeCell mixer
or free a context while registrations might call it. Expose a CoreAudio retry
adapter over the same injected retirement operation. A successful retry uses
existing StoppedMixerSource extraction after stop proves retirement.

Success leaves applied layout/settings, telemetry, absolute frame mapping,
start/stop and callback rendering unchanged. No new callback allocation/lock or
OS-specific business policy. If a caller discards an unretired pending owner,
the existing Drop safety leak may remain necessary; this API allows retention
and retry instead. Panic and native callback behavior are not proven here.

Author independent portable pending-owner state tests with real Mixer/queues/PCM,
opaque original/cleanup errors and Drop probes, plus macOS-only pure preflight
fixtures with no MachClock/native calls. The latter remain uncompiled by the
four allowed Linux/WASM checks. Assertions/runtime/device/formal review/QA remain
deferred; scoped Rustfmt and four sequential compile-only checks follow both
writer terminal stops. ASIO recoverable prepare, automatic live handoff and full
BMS player completion remain pending.
