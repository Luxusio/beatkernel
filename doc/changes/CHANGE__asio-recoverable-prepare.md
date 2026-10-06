# Recover ASIO prepare ownership with explicit callback retirement

Portable ASIO renderer construction returns the original mixer on setup refusal.
Native recoverable prepare retains available mixer or the actual failed stream.
The bridge reports callback retirement independently of driver cleanup errors,
allowing recovery after confirmed detach/drain while keeping original prepare
and cleanup diagnostics. Unproven retirement retains pending ownership.
Callback context destruction is likewise conditional on retirement evidence;
discarding an unproven owner retains its allocation instead of freeing storage
that native routing might still access.

## Evidence

Nine independent tests are authored: two portable renderer ownership cases,
three core cleanup-diagnostic cases, two SDK-gated scalar retirement cases and
two SDK-gated channel-preflight cases. Opaque errors, pending ownership and
actual queued/paused PCM are preserved without fabricated native controls or
callback contexts. Implementation integration is authored.
Existing APIs and native error priorities remain compatible. Core diagnostic
attachment and retirement ownership are static and cold; no render callback
allocation, lock, dynamic dispatch, SDK/dependency or license change is introduced.
Assertions, runtime, formal review and QA remain deferred.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
Five new portable tests compiled in the host workspace check. SDK-gated source
and four fixtures, the C++ ABI and native drivers remain uncompiled/unverified.
No tests or fixture effects ran. Existing unused-code warnings remain.

## Known ceiling

Windows/MSVC/SDK C++ ABI and gated source/tests are not compiled by the four
permitted Linux/WASM checks. Driver/callback retirement remains unverified.
SDK ABI is uncompiled; unproven consumed owners cannot recover and retain context for safety.
Recovery retains current mixer state, not guaranteed pre-prime cursor state or
unheard-buffer delivery. A consumed handle without retirement proof cannot be
treated as safely retryable. Automatic application handoff remains pending.
Discarding such an owner can intentionally retain context/PCM allocations for
callback safety. Device/native retirement proof remains required for recovery.
Full BMS player Goal stays active and incomplete.
