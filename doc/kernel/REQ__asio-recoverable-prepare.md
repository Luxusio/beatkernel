# Retain ASIO mixer and failed prepare ownership

AsioBlockRenderer exposes new_recoverable alongside compatible new. Invalid
frame/channel/capacity/storage preparation returns MixerOpenFailure with the
original renderer error and unchanged mixer. Native ASIO preparation stages
rate/buffer/channel probes before transferring mixer ownership, then uses this
constructor. Old prepare and prepare_with_clock signatures return the original
error. New recoverable variants retain mixer or the actual partial stream under
the shared OutputOpenFailure contract.

The bridge reports callback retirement separately from native cleanup status.
For valid same-thread close, disable callback admission, detach global routing
and drain admitted readers before reporting retirement. Driver stop/dispose/
Release errors remain original diagnostics and do not erase proven callback
retirement. Bad arguments/wrong thread do not claim retirement. Keep the legacy
close entry point and result semantics. Rust close outcome interpretation is
explicit about missing/invalid retirement evidence; no false success.
Callback retirement proves routing/storage quiescence, not successful driver
stop, disposal or unload. Preserve cleanup errors for later application policy;
this change does not authorize automatic ASIO reopening after those errors.

After buffer creation, latency query or B-priming failure, attempt close. Extract
the renderer mixer only after reported callback retirement and absent control.
Keep the original prepare error and cleanup diagnostic separately. If retirement
is unproven, preserve the actual stream/context as a pending owner; no unsafe
early extraction or synthetic model. A consumed handle without retirement proof
remains unavailable to ordinary retry rather than pretending close succeeded.
Expose a retry adapter over existing retirement state/StoppedMixerSource.
The callback context uses explicit drop ownership: only confirmed callback
retirement may free it. If a pending owner is discarded without that proof,
retain its context allocation rather than freeing memory behind native routing.
Normal close still drops the same Box after retirement; this adds no render work.

OutputOpenFailure supports attaching a known cleanup diagnostic without
allocating, replacing the original error or changing owner/mixer state. This
preserves errors even when native cleanup reports failure after safe retirement.
Device rate/buffer, absolute render identity, cadence and callback processing on
success remain unchanged. No real-time allocation/lock/dynamic dispatch or SDK/
dependency/license changes. Priming recovery retains current software state;
do not claim pre-prime cursor rollback or unheard-buffer delivery.

Author independent portable renderer failure/PCM ownership and diagnostic state
tests, plus SDK-gated pure close-evidence/preflight cases that call no driver,
registry, COM or QPC. The Windows/MSVC/SDK C++ ABI and gated tests remain
uncompiled by the four permitted Linux/WASM commands. Assertions/runtime/formal
review/QA remain deferred. Scope formatting and four sequential compile-only
checks follow both paired writer terminal stops. Full BMS player Goal and live
application handoff remain incomplete.
