# Browser mouse and pen canonical packets

The browser physical-input component gains genuine core Pointer and Button
encoders for mouse and pen sources. Absolute coordinates and relative deltas
remain distinct from button transitions, touch contacts and keyboard events.
Packets retain full source/sequence identities, the original HOST acquisition
clock, independent native event and control codes, and distinct mouse/pen
Native namespaces. Field snapshots are validated before bytes are written.
The contract lives in
[REQ__bms-browser.md](../kernel/REQ__bms-browser.md#canonical-mouse-and-pen-packet-component).

## Source evidence

`physical-input.mjs` exports `encodePointerEvent` (77-byte tag 3) and
`encodePointerButtonEvent` (69-byte tag 0). The existing native control, metadata
and position layout in `crates/beatkernel/src/input/codec.rs` supplies the wire
contract. No Rust, page, Worker ingestion or existing keyboard/touch/HID
behavior changed in this slice.

An independent author preserved all seven existing groups in
`physical-input.test.mjs` and added four groups for literal packets, both
namespaces and integer/f32 boundaries, malformed DTO refusals, and single-read
getter snapshots with owned output bytes. Total source groups: eleven. Both
writers reached terminal STOPPED before source inspection. `git diff --check`
returned exit 0; these groups were not executed. This JavaScript-only slice ran
no parser, Cargo check, browser/device/network operation or formal review/QA.
The full player Goal and Harness task remain open.

## Known ceiling

This is a basic packet component. Window acquisition, allocated source
ownership, coalesced histories, pointer profile routing, Worker batch ingestion
and live/replay/competition integration remain required follow-up work. The
core Pointer variant carries position/displacement and mode; it has no pressure,
tilt, contact or button-state fields. Buttons use their actual separate variant.
No page support, physical input acceptance or performance result follows from
the presence of these encoders. Browser execution and test assertions remain
deferred at the user's instruction.
