# Runtime hazard report delivery

RuntimeReport exposes hazard outcomes separately from normal judged note
results. Each successful bound-input judge call contributes its current hazard
report immediately; explicit and logical-end advances do the same. Binding
order and already committed prefixes survive later fanout failure. Rejected
calls and unbound/ignored input never replay a retained previous hazard report.
Marker identities, times, opaque values, outcomes and original input provenance
pass through unchanged, including existing clock-normalization evidence.

Existing ordinary judgment, sound publication and score telemetry retain their
semantics. Hazards do not implicitly enqueue note sounds or increment ordinary
judge-result counters. Audio queue failure preserves committed hazard outcomes.
Group and solo wrappers retain the same report, and no-hazard engines expose an
empty vector. External literal RuntimeReport constructors must now initialize
the new `hazard_events` field. The contract lives in
[runtime hazard delivery](../kernel/REQ__runtime.md#hazard-outcome-delivery).

Six independent deferred groups in `crates/beatkernel/tests/runtime_hazards.rs`
exercise first-boundary fanout and stale report exclusion, resolver-failure
prefixes, clock normalization and actual TouchRouter ownership, finite endpoints
and overflow, ordinary audio queue failure and score isolation, and actual
ReplayRecorder/ReplaySession reconstruction plus restored runtime owners.
Tests are authored for later execution.

After both writers returned actual terminal STOPPED, scoped Rust formatting and
whitespace checks completed successfully. Four locked compile-only checks exited
0: workspace/all targets with WebTransport, no-default WebTransport/all targets,
WASM browser/lib and WASM browser-audio/lib. Existing WASM cadence unused-code
warnings remain. No tests or runtime acceptance were executed.

## Known ceiling

This report component does not prepare BMS hazard plans, apply gauge/death,
play WAV00, render mines, compute mine completion or enable mine source
admission. Those live/local/replay/practice/offline integrations remain required.
No runtime/device/browser/performance or formal review/QA acceptance is claimed;
the whole player Goal and task remain active.
