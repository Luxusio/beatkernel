# Multi-host reported score display

Accepted room score prefixes now feed a portable retained `RoomOpponentHud`
through the browser Worker and the actual `BrowserLocalGame` bindings. Frozen
Prepared membership defines host/player identity and row order. The local host
is excluded; up to 64 hosts with 64 players each produce at most 4,032 remote
rows and 1,008 four-entry pages. Full u64 host identities, prefix sequences and
counters are preserved. Invalid, stale, reordered and post-final updates refuse
atomically; a disconnected HUD retains its last accepted rows.

The existing canvas renderer borrows only the visible page. Labels and counter
strings update on accepted prefixes rather than being reformatted each frame.
The clipped footer occupies y=646..720 below the existing local fields ending
at y=640. Neither room membership nor page changes alter note or touch geometry.
Window previous/next controls use correlated user-event Worker requests and
bounded page metadata; no periodic score payloads or Window gameplay rendering
were added. Keyboard, touch/pointer, HID and Gamepad acquisition retain the
Window/Worker ownership rule documented in `REQ__bms-browser.md`.

Presentation failure disables this HUD while allowing local judgment, capture,
audio and room networking to continue. Delayed final-drain callbacks may retain
network summaries but cannot access a disposed game or its HUD. Remote reports
remain untrusted competitive display, not local judgment or ranking authority.

Independent deferred fixture sources add four portable Rust groups, two Worker
groups (93 total) and one Window host group (95 total). They cover qualified
identity, maximum membership, pagination, atomic refusal, final/disconnect
behavior, actual footer composition, unchanged touch geometry, binding failure
isolation and disposed-owner callbacks. No fixture, JavaScript parser, browser,
application, generated binding or device run was executed.

After both writers stopped, scoped rustfmt and whitespace inspection completed.
Four compile-only Cargo checks exited 0: workspace/all-targets with WebTransport,
runtime no-default/all-targets with WebTransport, WASM browser, and WASM
browser-audio. The WASM checks retained three existing cadence dead-code warnings.
This is source and compilation evidence only. Native multi-host application
activation, native HUD/final-drain integration, generated WASM bindings, live
TLS/browser/device behavior and measured performance remain pending. The full
player Goal and Harness task remain open; no runtime PASS or close is claimed.
