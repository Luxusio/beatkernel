# Keep failed CoreAudio callback owners available for retirement retry

CoreAudio opening retains the original mixer before callback-context transfer.
After IOProc/listener registration failure it attempts the existing retirement
path and preserves the actual partial stream if cleanup refuses. A portable
OutputOpenFailure retains the original error, cleanup diagnostic and pending
owner, publishing recovered mixer state only after successful retirement.
Existing open error signatures and callback Drop safety behavior remain intact.

## Evidence

Six independent tests are authored: four portable pending-owner cases and two
macOS-only pure preflight cases. They preserve opaque original/cleanup payloads,
pending-owner Drop ordering, repeated refusal diagnostics, queued paused PCM,
explicit missing recovery and ownership moved out through into_parts.
Implementation is authored. The generic failure owner adds no allocation, lock or native IO;
retirement effects are supplied through static injection. Assertions/runtime,
formal review and QA remain deferred. macOS code/fixtures are not compiled by
the four existing Linux/WASM configurations.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The four new portable ownership tests compiled in the host workspace check;
macOS source and its two fixtures remain uncompiled by these targets. No tests
or fixture effects ran. Existing unused-code warnings remain; native callback
retirement, registration cleanup and device behavior are unverified.

## Known ceiling

Device-global rate/buffer changes are not rolled back. Discarding an unretired
owner may still require the existing callback-context safety leak. Native
registration, drain, cleanup retry and physical timing remain unverified. ASIO
recoverable preparation is subsequently extended in
[ASIO recoverable prepare](CHANGE__asio-recoverable-prepare.md); SDK acceptance
and automatic live application handoff remain pending.
macOS lifecycle acceptance, device-setting rollback and automatic handoff remain pending.
Full BMS player Goal stays active and incomplete.
