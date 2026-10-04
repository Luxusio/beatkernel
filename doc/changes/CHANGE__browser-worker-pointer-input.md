# Worker mouse and pen physical input

The browser Worker gains explicit mouse/pen setup and genuine Pointer/Button
batch ingestion through the shared physical input path. The numeric setup
owns descriptors and exact binding rows before chart construction, rejects
source collisions with HID/Gamepad, and shares the existing aggregate binding
budget. Local members receive only their assigned source; position-only rows
cannot prove press coverage. Admitted descriptors return in the actual prepared
receipt. The contract is
[REQ__bms-browser.md](../kernel/REQ__bms-browser.md#worker-pointer-setup-and-canonical-ingestion).

Whole-batch validation checks source/type/control admission before any Runtime
input call. Original timestamps, complete source/sequence bits, native codes
and Pointer modes flow through the actual canonical packet encoders and common
`input_blob` method. Existing chronology, fanout, watermark, recording and
competition ownership remain authoritative. No new platform-specific protocol
or separate judgment implementation is introduced.

## Source evidence and deferred checks

`pointer-profile.mjs` owns the bounded snapshot and exact Native rows.
`worker.js` validates optional `play-start.pointerSetup`, constructs solo/local
bindings, returns admitted `pointerDevices`, preflights mixed packets and clears
admission during disposal. Existing generic Rust `BrowserGame.input_blob` and
`BrowserLocalGame.input_blob` decode the original variant and retain the common
Runtime/capture path; no Rust or page code changed in this slice.

The independent author added three model groups and four actual Worker groups.
`pointer-profile.test.mjs` contains 3 groups, `play-worker.test.mjs` contains 107
(103 preserved), and preview `worker.test.mjs` keeps its 14 groups with the actual
new module linked. Fixtures cover ownership/limits, solo and exact local rows,
packets and mixed chronology, source collisions/replay refusal/combined budgets,
whole-batch rejection, expanded Gamepad fanout and retired-owner isolation.
Generated owners and browser APIs remain fixture substitutes; this does not
establish actual Rust judgment or browser execution.

Both writing lanes reached terminal STOPPED before final source inspection and
`git diff --check` (exit 0). No tests, Node/parser, Cargo checks, app/browser/device,
network or formal review/QA ran in this JavaScript-only slice. The broader player
Goal and task remain active; required browser QA still precedes eventual close.

## Known ceiling

This change connects actual Worker preparation and ingestion. Window event
acquisition, allocated source ownership, page controls and profile forwarding
remain required follow-up work. Ordinary BMS tap judgment does not gain pointer
position semantics; only actually bound inputs enter the existing capture path.
Pressure and tilt are not fields of the current Pointer packet. Browser and
device execution, generated bindings, actual replay/competition results and
input latency acceptance remain deferred. Source fixtures are not executed
acceptance or full-player completion.
