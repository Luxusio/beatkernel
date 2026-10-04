# Aspect-preserving player viewport

The shared native/browser renderer contains its logical scene within a centered
pixel viewport. Menu pointer conversion and pixel scrolling use that same
portable geometry. Browser Window forwards original CSS touch samples and cached
backing dimensions; Worker/WASM projects through the common viewport before
existing contact admission. This keeps lane drawing and input routing together
without adding a main-thread gameplay renderer or rewriting acquisition time.

Bars cannot activate menu controls. Captured touches project outside the scene
when necessary, so genuine release/cancel still clears the original admitted
owner. Whole-pixel fitting bounds the viewport on nonzero surfaces, handles odd
remainders deterministically, and necessarily quantizes extremely small views.
Existing explicit logical-point input APIs retain their caller-owned semantics.

Worker preflights every touch in a complete batch through the same allocation-free
scalar Rust projection before any event is admitted. This adds one touch-only
WASM validation crossing, without copying/decoding the packet or allocating a
projected-point array. Actual admission decodes the original packet and repeats
the same geometry calculation. Extreme positive-but-unrepresentable CSS geometry
therefore rejects before an earlier input in the batch can mutate gameplay.

## Verification state

Independent deferred fixture source adds eight groups: two portable viewport,
two canonical touch/core routing, one native input, two actual Worker and one
actual Window host group. They cover bounded fit and inverse mapping, bars,
extreme products, raw encoded provenance, held release/cancel through resize and
page remapping, native click/wheel alignment, whole-batch geometry refusal,
actual cached CSS/backing acquisition and lost capture without layout/render
work in the callback. Existing groups remain; play-Worker has 97 and play-host
98 source groups. No fixture has executed.

Both source writers actually stopped before scoped Rust formatting and
whitespace inspection. Workspace/all-targets with WebTransport (session 71287),
native no-default/all-targets with WebTransport (71739), WASM browser (31177) and
WASM browser-audio (33467) each exited 0. WASM retains three existing cadence
dead-code warnings. No tests, JavaScript parsers, applications, browser/device
execution, generated bindings or formal review/QA ran. These are Rust
compile-only checks; the full player Goal and Harness task remain open.

## Known ceiling

Actual GPU/native/browser/DPI/resize and physical touch acceptance is unverified.
An acquisition snapshots the current requested backing extent; a queued resize
can precede its actual presentation. This records observed host geometry and
does not claim an exact displayed-frame or physical sensor timestamp. Tiny
surfaces quantize the aspect ratio to whole pixels. This change does not alter
gameplay timing, native input acquisition or renderer resource limits.
