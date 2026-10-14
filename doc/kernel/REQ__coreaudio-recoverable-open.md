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

Legacy success leaves applied layout/settings, telemetry, absolute frame mapping,
start/stop and callback rendering unchanged. No new callback allocation/lock or
OS-specific business policy. If a caller discards an unretired pending owner,
the existing Drop safety leak may remain necessary; this API allows retention
and retry instead. Panic and native callback behavior are not proven here.

Author independent portable pending-owner state tests with real Mixer/queues/PCM,
opaque original/cleanup errors and Drop probes, plus macOS-only pure preflight
fixtures with no MachClock/native calls. Execute portable production logic and
focused regressions, then independent review and QA; cached macOS C-stub checking
provides Rust typing only. Actual HAL/device behavior remains separately required.
ASIO preparation follows
[ASIO recoverable prepare](REQ__asio-recoverable-prepare.md); SDK/device
acceptance, automatic live handoff and full BMS player completion remain pending.

## Complete retained Source ownership

The complete CoreAudio opener accepts NativeOutputState and returns it, or the
actual pending CoreAudioStream, through OutputOpenFailure on every failure.
Retain Mixer, prepared remix, generated PCM/report, admitted prefix and original
Source history identity. Its created frame basis is the first unsent frame;
neither the Mixer pull frontier nor created metadata grants native progress.
Keep legacy Mixer and remixed recovery compatible without flattening a complete
owner through into_mixer. Use one private Legacy/Complete render owner and one
native registration/start/stop/unregister/drain lifecycle.

Read nominal buffer frames and the optional native variable-buffer maximum off
the callback. Reserve only the actual prepared maximum, within the immutable
Mixer limit. Retained app composition must allow bounded buffer growth rather
than fixing that limit to the initial nominal buffer. Cold opt-in history growth
preserves every span, original identity, frontier and retirement revision, never
shrinks and commits capacity only after successful allocation. Existing legacy
history preparation/telemetry semantics remain unchanged. Construct complete
telemetry after this preparation with the explicit first-unsent basis.
Native presentation validation accepts a positive observed callback up to that
cold advertised maximum while preserving the exact applied nominal metadata.
The explicit maximum cannot be zero or below nominal. Preserve every original
Mach/sample/flag/grid/domain check and the legacy nominal-only helper behavior;
never rewrite nominal settings to make an oversized observation appear valid.

Replacement observation accepts native future presentation as pending work,
retaining the first two progressing original SourceFrame packets until their
HOST endpoints are covered. Use the caller's poll clock unchanged. Current
stream/packet errors remain visible before any cached release; stopped or
authorized streams cannot qualify cached evidence. Original epoch, basis,
Source identity, domains, revisions and submission chronology must match actual
output expectations, including the first and full-cache observations. Releasing
an older packet must not permanently discard a later packet merely marked as
seen while the cache was full. Timeout, cancellation and stale-evidence refusal
remain owned by the common replacement controller.

The sole callback retirement decision precedes configuration-error accounting,
history, rendering, PCM copies and outer publication revisions. Authorized
retirement returns without touching HAL's documented initially zeroed output;
silence is not admitted Source PCM. Authorization does not prove native drain.
Preserve checked nonces, busy/unconsumed outcomes, cancellation versus
authorization in one CAS and sticky authorization. A single control producer
publishes requested fixed geometry before the pending request; an older request
cannot authorize using a later payload because it must CAS its exact nonce.
After authorization, is_started must no longer qualify the stream as live.
Source reads check authorization before and after their coherent read, because
retirement deliberately leaves the last even publication unchanged. Refuse new
history intents when authorization is already visible; an intent admitted before
that check remains queued and is not retracted by a later authorization. Failed
Start also prevents public Source qualification and history admission until
retirement; historical snapshots remain available only as diagnostics.

Validate every channel group, count, alignment, byte extent and overlap before
any output write, then perform infallible copies. Render actual callback frames
only when no block is pending; otherwise copy the original pending prefix and
retain the original report. Insufficient pending PCM refuses intact, without
padding, rerender or invented admission. Any genuine post-write uncertainty
quarantines the complete owner against ordinary replay/reopen.

Track a native start attempt separately from a successful start acknowledgement.
Failed Start may have executed callbacks; Stop/unregister/drain must succeed
before complete owner extraction. Preserve the original operation error and
available cleanup error independently. Native rate/buffer settings are global
effects and are not restored by source rollback or ownership recovery.

Portable tests execute the same production decision/render/copy/admission step
with real Mixer queues/history/nonzero PCM and allocation probes. Actual app
backend, typed Source UI and solo/local hooks must compile through macOS code.
Keep native lifecycle sequencing in one statically injected private implementation:
production supplies HAL Create/listener-add/Start/Stop/listener-removal/IOProc-destruction operations,
and cfg-macOS fault fixtures invoke that same sequence with controlled failures.
Prepare failed-start callback work, retry and exact error/ownership assertions
even when this host can only compile them. Do not replace native behavior proof
with a parallel test-only lifecycle model or claim C-stub execution.
Neither this contract nor host tests/foreign typing establish actual native
registration, timing, cleanup failure behavior or acoustic acceptance.
