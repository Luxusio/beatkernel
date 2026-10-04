# Browser room committed-start bridge

The WASM room binding and Worker room owner use the common admission/clock/start
client with original captured observations and exact write receipts. Worker
retains actual Window clock origin and preparation preroll, then translates one
committed schedule into the original host clock. Actual committed, live and
future output-frame-rounded evidence replaces the unconditional room activation
fence; setup snapshots and queued readiness still cannot authorize output.
Handshake expiry, one-shot schedule delivery, stale callbacks and joined
cancellation/free ownership remain explicit. The Window keeps input/required
browser acquisition and event-driven control updates; rendering stays on Worker.

Independent deferred owner/Worker fixtures cover timing, schedule authority and
cleanup: three additional groups each, with 11 Owner and 84 Worker groups total.
After both writers stopped, scoped Rust formatting, whitespace checks and four
compile-only Cargo checks succeeded: workspace and headless WebTransport paths,
WASM browser and WASM browser-audio paths. Existing WASM cadence dead-code
warnings remain. No JavaScript parsing, test/browser/network/audio execution,
generated bindings or review/QA acceptance are claimed.
Page lobby, native timed adapters, participant progress/final ACK and actual
native/browser/output interoperability remain required. Full Goal remains active.
