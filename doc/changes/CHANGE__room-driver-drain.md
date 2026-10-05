# Browser drain uses the common room final policy

The browser's split-operation Rust driver records successful final-prefix and
drain admissions separately from real write/ACK/completion receipts. Its finite
drain step delegates to RoomFinalWaitState, transferring existing final evidence
without resending it. The WASM binding exposes exact integer setup/step results;
the Worker room owner schedules pending steps and resolves one Promise from
common-policy completion plus validated receipt observations. Transport writes
still earn credit only from actual asynchronous completion with original IDs.

## Known ceiling

JavaScript still owns timers, Promise/callback lifecycle and transport cleanup.
Full RoomCompetition/RoomNetworkActor browser orchestration is unfinished. Pure
fixtures use actual common protocol operations where possible; JS adapter
fixtures script the binding boundary and do not prove Rust protocol admission.
Tests, JS parsing/runtime, generated bindings, WebTransport/browser acceptance,
performance measurement, formal review and QA remain deferred. Compilation is
source compatibility evidence, not end-to-end gameplay or physical sync proof.

Three additional pure driver fixture groups (ten total) cover atomic setup and
fixed deadlines, clock/Leave/close refusals, and genuine common protocol final
uploads, peer/aggregate ACKs, full DrainReady writes and DrainComplete admission.
Four added actual-owner JS groups exercise scripted step authority, malformed
results/missing receipts, timeout/cancellation and refusal settlement. Legacy
mock compatibility is a scripted binding edge, not a protocol proof.

Both writers stopped before scoped Rust formatting and four sequential
compile-only checks, all exit zero: workspace/all-targets with WebTransport,
no-default-features WebTransport/all-targets, WASM browser/library and WASM
browser-audio/library. Existing unused-code warnings remain. Host checks compile
pure fixtures; WASM checks compile actual binding methods but not fixture
children or generated JS. JS files were inspected as text only, never parsed
or executed. No assertions or formal acceptance gates ran.

A rejected drain that leaves the transport alive must not reenter its sealed
policy on later receipt events. The settled guard and independent adapter
fixture pin this lifecycle rule; successful settlement also prevents later
steps. This source correction is not a formal reviewer verdict.
