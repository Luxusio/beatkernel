# Native committed mine presentation

Native solo and local presentation snapshots retain each player's committed
MineDamageSummary alongside ordinary score and pressed state. A sole local
member supplies the compatibility snapshot; multiple members retain their own
summaries without a fabricated aggregate. Empty reports preserve prior damage.
Pause, cancellation and final publication retain admitted damage evidence.

Solo and local report publication preflights damage together with score and
input before committing member state. Local damage preparation uses bounded
stack storage for at most 64 members. Invalid damage and overflow must not
publish a partially updated batch.

Native replay publication accepts the ReplayVisual owner's absolute cumulative
summary at both normal and pause boundaries. Equal prefixes are idempotent;
regressing or numerically impossible summaries are rejected before mutation.
Legacy replay publication without mine metadata preserves the previous summary.

Four independent deferred fixture groups use actual Runtime/RuntimeGroup
reports and public player channels to cover solo/local retained damage, whole
local batch atomicity and absolute replay publication. Both writers returned
actual terminal STOPPED before scoped formatting and compile integration.
Formatting and whitespace checks completed; four locked compile-only checks
exited 0: workspace/all-targets with WebTransport, headless WebTransport/all-targets,
WASM browser/lib and WASM browser-audio/lib. Existing WASM cadence unused-code
warnings remain. Windows/macOS conditional native code was not target-compiled.
No test execution, device/browser/performance acceptance, formal review or QA
is claimed. Gauge policy, playback failure/stop, WAV00 and mine drawing remain
separate unfinished work; mine source admission remains guarded.
