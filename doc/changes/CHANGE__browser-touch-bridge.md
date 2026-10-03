# Browser Window touch to common runtime

The live page gains an explicit contact policy, automatic on touch-capable
PointerEvent browsers, while keyboard remains available. Replay uses recorded
rules. The policy selects contact judging and preserves that rule identity;
it is not a device picker or a compatibility conversion.

Window acquires genuine touch contacts with original timestamps, native pointer
provenance and adapter contact IDs, retains capture, and shares the bounded
queue and sequence with keyboard input. Cached CSS extent avoids input-callback
layout reads. Worker validates the full batch, encodes canonical Touch events
and projects hit coordinates separately before common runtime admission.

The default renderer and touch setup share logical dimensions and exact integer
lane partition. Contact ownership stays with the original lane until matching
release/cancel, including movement outside that lane. Packets and captures keep
original coordinates and acquisition time. Missing required capabilities fail
explicitly and do not fall back to keyboard emulation.

Ten new fixture groups are authored: two portable Rust groups, three canonical
helper groups, three Worker groups and two Window groups. Existing groups remain.
Workspace all-targets, headless all-targets, WASM browser and WASM browser-audio
`cargo check` each exited 0 after both writers stopped. WASM retains the three
existing cadence dead-code warnings. Compilation checks include the Rust fixtures
but execute no assertions; JavaScript fixtures were not executed or parsed. Runtime tests,
generated bindings, browser/device acceptance and performance measurements are
deferred. Contact pressed feedback, WebHID permission/report decoding, catalog
mode migration and native/contact competition remain further work.
