# Browser control layout containment

Baseline `9c6fcb8` had measured 429px document width at a 390px viewport,
with numerical-preset and saved-record native selects extending outside their
panels. Source `d03670b` constrains label/control intrinsic sizing, permits
horizontal labels to wrap and bounds binding/source grid minimums to available
width. Native HTML structure, state, focus rules and Worker/business/audio owners
remain unchanged; no JS observer, render loop, library or VDOM was added.

`git diff --check` passed and note freshness invalidated no dependent notes.
Independent full planning strengthened the browser matrix with control/panel
bounds, expanded details, long labels, keyboard access and desktop comparison.
Independent DEEP source review and both discovery hunters found no actionable
defect. Actual browser QA recorded 84 valid geometry snapshots across initial,
prepared, error, expanded two-player and separately disclosed synthetic long-label
states, including exact baseline CSS comparisons. Current controls and document
extent fit all seven widths (320/390/599/600/601/800/1100px), with one-pixel
rounding tolerance. Six file-input families and actual generated source/opponent
target fields were covered. At1100px baseline/current widths match for file290,
rate140, seed202 and gauge123px. No unchanged Rust/Node suite or WASM rebuild was
needed for this static stylesheet correction.

Actual native Tab/Shift+Tab, select arrows, Space and Enter passed. The ordinary
journey prepared a real chart, completed a1s capture, saved it, refused an
incompatible section without losing the opponent, corrected/retried/completed,
returned through the original Settings parent and completed a saved replay.
Independent UX passed expanded320/390/800/1100px controls and actual keyboard,
replay, refusal/recovery/save and menu Back. Screenshots were opened and inspected.
Evidence: `target/wf/browser-controls-responsive-layout/qa-browser-1/` and
`ux-browser-1/`, including original driver failures and their follow-up observations.

The first baseline inline-style injection was blocked by CSP; its measurements
are invalidated and preserved. Corrected baseline observations intercepted the
same-origin stylesheet request with exact Git bytes and checked computed styles.
An unsupported Shift+Tab driver chord was corrected with explicit modifier keys.
A supplemental second-tab renderer initialization/disposal timeout did not supply
genuine workflow evidence. UX's offscreen final menu click likewise timed out;
ordinary scrolling and pointer interaction on the same page recovered. These
driver/observation limitations do not become product PASS or FAIL claims.

The actual qa-browser and ux-browser finals both passed. All observation/UX
drivers disconnected, and QA joined its owned browser/server/Xvfb, removed the
temporary profile/cache and confirmed host45062 terminal. Closure still depends
on the final Harness receipt gate; no receipt files are manually authored.

## Known ceiling

Canvas bitmap font/readability, dense settings, technical refusal wording, full
player UI, physical input/audio timing and broader WBS acceptance remain
unfinished. No whole-player performance improvement
or WBS promotion is claimed; WBS remains 90/193. Task closure separately requires
Harness-owned ordered review/QA receipts.

Contract: [browser control layout](../ui/REQ__browser-control-layout.md).
