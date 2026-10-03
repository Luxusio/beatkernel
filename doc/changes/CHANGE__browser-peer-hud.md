# Worker-owned multiplayer peer HUD

Actual peer prefixes and connection state are retained on Worker and rendered
by the common competition scoreboard beside saved opponents. Normal peer
updates retain the existing coalescing cadence and no longer update live Window
DOM counters. The numeric HUD boundary preserves signed song time and full u64
counters using common progress validation. Peer scores remain reported data,
independent of local judgement, output timing and authenticated scoring.

Peer and saved display failures preserve the other component. Presentation
failure cannot replace local results or actual transport write/ACK evidence.
Correlated stop/error receipts retain the last received peer prefix and final
prefix flag; Window may display these once after joined cleanup for the current
session. Absent and stale prefixes cannot become a fabricated score.

Production source and seven independently authored deferred fixture groups are
written: portable HUD +3 (6 total), Worker +2 (47 total), Window +2 (56
total); preview keeps its 14 groups with capability mocks aligned. The Rust
fixtures preserve shared validator coverage in headless configuration and gate
only common graphics assertions on the graphics feature. Scoped Rust formatting
and whitespace checks are complete. Permitted cargo check commands completed
with exit 0 for workspace/all-targets, headless/all-targets, wasm32 browser/lib
and wasm32 browser-audio/lib. The two WASM paths retain the three existing
platform cadence dead-code warnings. No fixture assertions were executed.
JavaScript parsing, tests, generated bindings, browser/network/device/runtime
execution, formal reviews and QA remain deferred. The full player Goal and
measured rendering/input/main-thread acceptance remain outstanding.
