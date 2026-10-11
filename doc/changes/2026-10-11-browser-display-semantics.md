# Browser Display semantics

Status: implementation candidate; independent review and actual CLI/browser QA
remain pending. WBS stays 91/193; this does not complete the player Goal.

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
Actual production browser geometry verification and independent full review/QA
are still pending. These development results are not independent QA PASS.

## Known ceiling

Physical GPU/device latency, physical OS IME, screen-reader completeness and
browser FPS/lookahead configuration are not established by this change. The
recurring audio-WASM cancellation remains an unfinished strict browser failure
in earlier tasks; no network/audio assertion is relaxed here.
