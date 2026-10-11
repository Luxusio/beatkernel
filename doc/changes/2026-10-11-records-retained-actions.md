# Actual retained Records acceptance — 2026-10-11

Records now publishes completed catalog changes while preserving the selected literal
record, retires Preview when record identity changes, and clears obsolete renderer
preview/history when an admitted full packet has no preview. Records accepts
ArrowUp/Down, Home/End and Escape through the existing menu action route, including
Escape from an empty catalog. Native editors, modified keys and gameplay retain input
ownership. No virtual DOM, new protocol or per-frame reconciliation was introduced.

Contract: [retained browser menus](../ui/REQ__browser-retained-menus.md#actual-records-selection-and-relative-opponents).
WBS: [11.05 / E42](../kernel/REQ__bms-player-work-breakdown.md).

## Changes and regressions

Base: `8978857`; verified source: `de8ee17`.

- `d3331c3`: current Records catalog publication and literal selection preservation.
  Ordinary development Scan showed stored/DOM IDs2,1 but retained fields1; fixed
  publication uses existing menu-fields with captured owner/revision/Worker fences.
  DOM store completion still works after navigation; Scan keeps strict ticket cancellation.
  New actual-main regressions: old main2 PASS/4 FAIL, current6 PASS. Private Rust
  guard fixtures cover pending mutation and exhausted revision without widening APIs.
- `664a3e4`: successful identity-changing selection/catalog updates retire owner
  preview and archive. Same-key reorder and refused mutations preserve state.
  Two generated-ABI regressions use actual BrowserLibrary/BMS+PCM preparation and
  BrowserGame capture; old bindings fail both, current full file10 PASS.
- `318aef8`: admitted full packet with absent preview clears renderer preview,
  details and page. Partial same-key updates preserve details. Genuine Rust fixture
  covers Own/Other membership, skipped intermediate snapshot, no-preview Remove and
  fresh Preview recovery. Ordinary rapid pointer reproduction is not claimed.
- `de8ee17`: Records keyboard intent adapter, existing menu-action dispatch and
  owner/token reentrancy guards. Pure keyboard12 PASS and new actual-main host5 PASS;
  previous main2 PASS/3 FAIL for those host regressions. Existing test prefixes remain
  byte-identical. No Rust/WASM source changed after `318aef8`.

Independent DEEP reviews first found the owner retirement defect, then the absent
full-packet renderer defect. Both were fixed before final affected review PASS.
QA1 browser/UX then found the missing keyboard connection; all pointer/history/catalog
positives were preserved. Keyboard was fixed and independently reviewed PASS before QA2.

## Final evidence

Scratch evidence root: `target/wf/records-retained-actions/` (ignored, local evidence).
Final required lenses returned actual PASS; these finals are NON-ATTESTING until
the runtime confirms ordered hook-owned receipts. No receipt or close is claimed here.

- `qa-cli-1`: full app Rust2,391 PASS/0 FAIL/6 existing ignored at `318aef8`.
  `qa-cli-2`: full Node931 PASS/0 FAIL/0 skipped at `de8ee17`; fresh keyboard/host,
  old-main red/green, syntax/diff and exact unchanged Rust/binding provenance checks.
  Rust results are inherited from byte-identical compiled source, not a fresh QA2 run.
- `qa-browser-2`: headed current browser; genuine completed IDs1/2, exported originals
  and archives decode to 95/98 accepted operations, start0/end/recordedUntil1e9.
  Timing count0 is honest miss-only capture; executed pure Rust covers nonzero timing.
  Genuine stored comparison/history connects to read-only packets and actual paint.
  Ordinary retained Preview/AddOwn/AddOther shows nonzero selected counts; duplicate
  refusal, exact-kind no-preview Remove, unrelated preservation and Clear pass.
  Save/delete/Scan preserve literal record2 across row changes. Keyboard navigation,
  native INPUT/SELECT/modifier ownership, exact Settings parent/thirteen fields and
  empty catalog row no-ops/Escape pass. Console/page/HTTP errors0.
- `ux-browser-2`: actual desktop1200×1000 and narrow390×844, 22 screenshots inspected,
  12 checkpoints, 268 observed trusted inputs. Keyboard, preview retirement, removal,
  genuine history/comparison, narrow Clear and exact parent/reopen pass.
- Independent original11.05 audit inspected source, actual artifacts/images and
  executed layer evidence; no remaining original criterion gap. Only 11.05 moves W→D:
  completed90→91/active193 =47.15%, remaining102. Other leaves remain unchanged.

Final browser WASM SHA256:
`2fbf27acd8f3d9a95eb33c7b1077ec491fe2fc8e87b820681f5402feb7ecbc3a`.
Audio WASM SHA256:
`dde5198fb9779d4f1f8c5927ca446f929ea1fcdab8c8ca1a15ebaf46f35453b8`.
Main SHA256: `38460e179b9f71645489f2a785e8ff747d88f95e707396adced99ab27235be92`.
Keyboard SHA256: `aa12ff9fa5623fcda9e6fc9cd2251718a31820bae590f2ec11e8b5867d45355d`.
Both release packages were rebuilt with installed pinned wasm-bindgen0.2.129.
All owned browser/server/Xvfb processes joined and profile/cache removed; borrowed
UX drivers disconnected with explicit handback. No validation process remains owned.

## Preserved failed attempts

Original development1 BigInt export driver error remains preserved. Corrected
development2 established the Scan defect; development evidence is not formal QA.
Initial binding generation exited127 because wasm-bindgen was absent from PATH;
using existing `target/toolchain/wasm-bindgen-cli/bin/wasm-bindgen` succeeded.
QA1 keyboard FAIL and positive pointer evidence remain under qa-browser-1/ux-browser-1.
Initial new host-test failure used a fixture tag equal to its ID instead of INPUT;
only that new test setup was corrected, preserving assertions and old harness.
QA2 scratch empty-delete loop initially counted the placeholder option; original
script/log were preserved, corrected stored-value termination passed on the same
empty page. Neither scratch driver error required a product change.

## Known ceiling

Existing bitmap text/small-font narrow readability, complete accessible semantics
and full UI parity remain Goal work. Authentic OS IME composition is unavailable in
the Xvfb browser setup; editor/modifier ownership has actual GUI evidence, while
composition/reentrancy/gameplay and hostile/stale/quota boundaries have layer tests.
Skipped-intermediate full-snapshot behavior has pure fixture evidence, not ordinary
pointer reproduction. Physical devices/audio latency, other platform execution,
world-leading performance and whole-player completion are unproven by this task.
Actual lens PASS and WBS software acceptance do not imply receipt-backed Harness close.
