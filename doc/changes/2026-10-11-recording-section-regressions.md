# Recording section compatibility and Worker regression fixtures

## Problem and implementation

A genuine finite-section browser recording was saved successfully but refused
when selected as an own saved opponent for the same section. Capture encoded
the configured end, whereas saved admission used the normalized network
header with no end. The repair preserves that public network basis and adds
a crate-visible capture-exact projection of the same canonical builder for
solo/local owners. Strict replay compatibility, existing bytes and network
endpoint envelopes remain unchanged.

Three Worker recording/result/archive fixture libraries were obsolete: their
missing declaration, incremental preparation and queued channel interfaces
prevented intended assertions from running. Each now follows the existing
play-worker preparation pattern, including delivery reservations, successful
setup preconditions and one-shot job cleanup. Original assertions remain;
completed-results also updates its fake sample to `take_pcm_array()` after
the first focused run exposed that next obsolete seam.

## Development and independent acceptance evidence

Base: `b65e749`; previous production source: `073a449`.
Implemented source: `7fb6a5a`; final regression fixtures: `1282130`.
Logs: `target/wf/player-record-worker-regressions/` (ignored local evidence).

| Check | Observed result |
| --- | --- |
| Untouched full Node baseline, concurrency 1 | 904 tests; 882 passed, 22 failed, no skipped/cancelled |
| Repaired three-file focused Node, concurrency 3 | 35 tests; 34 passed, 1 failed at obsolete sample fake API |
| Completed-results after sample seam repair | 9 passed, 0 failed, no skipped/cancelled |
| Shared library no-default-feature check | exit 0; configured native CC/AR |
| Step comparison focused first run | 4 passed, 2 failed: new local fixture lacked visible-lane bindings |
| Step comparison after genuine local binding setup repair | 6 passed, 0 failed; 24 actual capture/decode/admission cases in matrix |
| Existing finite/contact/local/timing/saved regression groups | 6 / 6 / 6 / 14 / 8 passed, 40 total, no failures |
| Actual WASM test initial baseline attempt | 9 setup failures: fixture incorrectly assumed a pending asset name must be unacquired |
| Actual WASM test corrected baseline | 4 open tests passed; 5 finite tests failed with genuine `IncompatibleSetup` |
| Actual WASM test with fresh updated browser bindings | 9 passed, 0 failed, no skipped/cancelled |
| Fresh browser and browser-audio release builds / bindgen | both exit 0 |
| Changed Rust formatting / diff whitespace | no new format debt in edited regions; diff check exit 0 |
| Independent DEEP code review and two discovery hunters | PASS; no actionable findings |
| Independent app library, desktop/webtransport features | 2,384 passed, 0 failed, 6 existing ignored; 2,390 total |
| Independent full Node suite, concurrency 4 | 913 passed, 0 failed/skipped/cancelled |
| Independent focused Node suite | 44 passed: genuine WASM 9, historical 22, results 9, archive 4 |
| Independent genuine baseline bindings | 4 open passed, 5 finite failed with `IncompatibleSetup` |
| Independent shared no-default check / reproduced WASM bindings | exit 0; both package hashes reproduced |
| Interactive browser QA | PASS; finite original / Own / Other completed and saved, mismatch recovery and replay checked |
| Independent interactive browser UX | scoped PASS; ordinary Own/Other/replay and keyboard/navigation checked |

Historical has 22 passing tests and archive has 4 from the focused run. The
remaining original nine completed-results tests passed after updating the
sample fake. Local fixture repair supplies actual exact-device lane bindings;
it does not weaken the game's membership/setup validation. Full fresh Node,
independent review and interactive browser acceptance have now passed.

Actual preparation may retain a pending asset name across processing stages.
The new binding fixture now follows production's acquired-file reuse and
asserts each asset was hydrated once at Ready. Corrected baseline failures
occur at actual saved-opponent admission, not at that preparation helper.

Fresh browser WASM SHA256:
`7961c9a057797ac21941cbb16d53f6938dd294e922574d81c588ebeada6822f8`.
Fresh browser-audio WASM SHA256:
`dde5198fb9779d4f1f8c5927ca446f929ea1fcdab8c8ca1a15ebaf46f35453b8`.
The latter remains byte-identical to the baseline after an actual rebuild.
Full-file formatter diagnostics include existing fixture/local-render debt;
the edited ranges introduce none (`scoped-format.json`). Original Rust fixture
prefix and all original Worker test names/order remain preserved.

## Known ceiling

Browser QA decoded three genuine saved `.bkr`/`.bkra` pairs with start 0 and
end 1,000,000,000ns. Own and Other comparisons completed and persisted with
their History metadata. A 1.5-second mismatch preserved the selected opponent;
correcting it to 1 second allowed completion. Final console, page and HTTP
errors were zero; the expected typed mismatch occurred once. Evidence is under
`qa-browser-1/acceptance-audit.json` and `transcript.md`. Initial BigInt export,
diagnostic field and scratch decoder mistakes remain in the original logs;
they were corrected against the same live ordinary-UI session. Owned browser,
server, Xvfb and UX drivers finished; temporary profile/cache were removed.

UX evidence under `ux-browser-1/` includes the initial field-selection driver
mistake and the corrected ordinary keyboard journeys. Existing backlog remains:
small/clipped bitmap labels, dense forms, generic mismatch guidance and 390px
horizontal overflow to 429px from existing select controls. These were not
changed by the recording repair. No whole-player performance or physical
audio/input claim is made. Native evdev acceptance remains unfinished.

Review and QA finals are substantive PASS; Harness closure additionally needs
ordered hook-owned completion receipts and has not yet been established. No
WBS promotion is made; WBS remains 90/193 (46.63%). Test scopes differ from the
previous cleanup task's all-target suite, so the Rust totals are not comparable.

The Worker preparation GUIDE captures the confirmed queued delivery and
repeated pending-path lessons. Browser evidence-driver improvements, old format
debt and visual backlog remain deferred; no unrelated source changes were made.

Durable contracts:
[saved section compatibility](../runtime/REQ__saved-record-section-compatibility.md)
and [Worker fixture guidance](../verification/GUIDE__browser-worker-preparation-fixtures.md).
