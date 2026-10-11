# Browser Display semantics

Status: implementation reviewed; CLI checks pass, but strict browser acceptance
remains unfinished after contradictory actual runs. AC-001 through AC-004 pass;
AC-005 is unfinished. WBS stays 91/193; this does not complete the player Goal.

Browser Display previously painted native UI FPS/lookahead labels while applying
CSS width/height. Its explicit browser presentation now describes those actual
fields and supported auto/fifo choices. Native Display retains its original
timing draft and help. Both mount the same declarative retained tree without a
virtual DOM or per-frame vocabulary conversion.

The shared four-field validator preserves admitted strings, including leading
zeros, and refuses malformed or unsupported drafts atomically. Worker calls it
before Done admission. Window requires the current Display token/route and
exact current-draft equality before publishing both dimensions and the frozen
last-applied values. Back/reopen copies those values; editing or Back alone
does not apply them. See the [Display contract](../ui/REQ__display-declarative-ui.md).

Development evidence: five pure validator tests, twenty Rust Display-related
tests, six actual-WASM Worker regressions and twenty-two Window boundary tests
passed. Two new fixture setups initially attempted an illegal Selection-to-
Display transition; they were corrected to Selection-to-Settings-to-Display
without weakening assertions, then their scoped suites passed. Original failure
logs remain under `target/wf/display-development-12b18/`. JavaScript syntax and
the changed Display view's scoped formatting checks passed. Matching browser
WASM rebuilt with SHA256
`ab1732c2308c34815c06e8c6bc6d83839835f1d347425962122010427b9fff47`.
These development results are separate from the independent results below.

## Independent verification

Frozen source: `fb6b1b69e7da1c95a7c4d3605e12791d16ce877c`. Independent deep
code review (including two defect hunters) and security review returned PASS
with no findings. Their actual results do not substitute for hook-owned
attestation: task context still reports missing review/QA completion receipts.

Independent CLI checks executed the full Rust graphics library suite: 2415
passed, zero failed, five ignored platform/environment tests. The full serial
Node suite's exact rerun passed 1028 tests, zero failed/cancelled/skipped.
Combined: 3443 passing tests, five ignored. Direct public validator checks,
JavaScript syntax, diff checks and final source/WASM identity also passed.
Evidence: `target/wf/display-qa-cli-12b18-1/`.

The initial full Node run ended with exit 143 after passing test 689, without
final TAP totals; the cause is unconfirmed. Its `node-full.log` is retained,
and `node-full-rerun.log` contains the successful exact rerun. Two initial shell
wrapper parse errors exited 2 before the intended commands executed; they
were corrected without changing product source or assertions.

Independent QA-browser returned actual PASS: all twelve production journeys
completed, with zero page, console or network errors. Actual Display CSS
resize to 1000 by 700, renderer acknowledgement, accepted-field reopening,
invalid-height refusal and repair to 960 by 720 were verified. Fresh Display
screenshots were inspected. Original trusted keyboard, live play, captured
two-input replay and playback checks also passed. Evidence:
`target/wf/display-qa-browser-12b18-1/evidence.json`.

The subsequent independent UX run used unchanged source/WASM and completed
the same twelve functional journeys. Its scoped Display UX result was PASS,
but its preserved strict probe returned FAIL: one audio-WASM fetch reported
HTTP 200 followed by `net::ERR_ABORTED`; page and console errors were zero.
Evidence: `target/wf/display-ux-browser-12b18-1/evidence.json`. Both browser
runs used desktop Chromium at DPR 1 and recorded owned browser, display and
server cleanup. Small hint text and below-canvas error placement remain UX
backlog observations.

The later contrary strict failure prevents AC-005 acceptance; the CLI agent's
aggregate verdict is FAIL despite passing CLI checks. No network assertion was
relaxed, no stable audio-loading fix is claimed, and this task is not closed.
The evidence directories are ignored local artifacts and may be absent in a
fresh clone; production probes and test sources remain the reproduction path.

## Known ceiling

Physical GPU/device latency, physical OS IME, screen-reader completeness and
browser FPS/lookahead configuration and non-unit-DPR execution are not
established by this change. The recurring audio-WASM cancellation remains an
unfinished strict browser failure in this task and earlier tasks. Diagnose or
isolate its cause and obtain fresh strict browser acceptance before completion.
