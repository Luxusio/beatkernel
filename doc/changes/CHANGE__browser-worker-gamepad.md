# Worker gamepad bindings and canonical physical input

Connect optional gamepad setup and samples to the actual gameplay Worker.
Bounded connection descriptors and exact-source binding rows preserve full
64-bit identities. The Worker adapter retains source control levels, emits
changed pressed/touched buttons and absolute analog axes in the Gamepad native
namespace, and submits genuine canonical packets to the existing Rust physical
input entry point. Ordinary press-chart coverage requires pressed-button
bindings; analog controls do not become keyboard events or imply axis judgment.

Whole-step draft preflight bounds fanout and avoids accepting malformed tails
after mutating adapter state. Repeated samples emit no events even when their
original timestamp predates the committed global watermark. Changed late input
is refused rather than retimestamped. Pre-origin levels are retained while
Runtime fanout is discarded, preventing synthetic late Down events. Existing
keyboard/touch/HID and physical replay ownership remain shared.

Known ceiling: Window page setup/forwarding is still pending, so this Worker
source integration does not establish playable page gamepad support. Browser
execution, device latency, capture/replay and main-thread performance remain
unverified. Polling cannot recover missed intermediate transitions, and the
existing global-prefix rule explicitly rejects changed late samples.

Seven independently authored deferred groups cover exact full-width bindings,
independent canonical packet literals/metadata, double-to-f32 boundaries,
atomic draft state, strict source/count/clock/configuration limits, 256-packet
admission, genuine Worker mixed blob routing, collision/mode/coverage refusal,
pre-origin retention, stale unchanged versus changed-late input, and fail-stop
ownership. Profile fixtures total 5, Worker fixtures total 57 with 55 retained,
and all 14 preview groups remain unchanged except import-loader alignment.
Scoped whitespace found no diagnostics. No parser, assertions, tests, Node,
generated bindings, runtime/browser/device/audio, formal review or QA was
executed; unchanged Rust checks were not repeated for this JS-only change.
The full player Goal and Harness task stay active and unproven.
