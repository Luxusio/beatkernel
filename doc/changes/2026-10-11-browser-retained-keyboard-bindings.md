# Retained browser keyboard bindings

Status: implemented; independent code/security review and CLI verification
passed, but overall QA is FAIL. AC-001 through AC-004 pass; AC-005 remains
unfinished because the strict browser network check fails. This does not close
TASK__browser-retained-binding-editor or promote a WBS item. Overall WBS remains
91/193 (47.15%); the full player Goal remains active.

Settings carries the existing eighteen physical keyboard codes after its sixteen
scalar/policy fields. Worker validates all 34 before Apply; Window publishes
accepted values and snapshots bindings before playback. Raw invalid drafts stay
editable. Legacy 13/16 Apply preserves bindings. The original DOM selectors and
reset are locked throughout Settings ancestry, including child screens. Full
version-one profile admission replaces all 34 rows through existing T0/T1/T2
fences; Practice Done changes only section values. Legacy replacement cannot
truncate a complete parent. No virtual DOM, profile-version change or gameplay
frame conversion is introduced. See the
[owning requirement](../ui/REQ__browser-retained-keyboard-bindings.md).

Independent results at source HEAD `86e48bd650527996621f988bb0a2910f6eb6eb0d`:

| Scope | Actual result |
|---|---|
| Code review | PASS after correcting the browser probe's captured Native keyboard record decoder; contract discovery found no further defect |
| Security review | PASS; subsequent two-line test decoder correction changes no product authority or validation boundary |
| Full Rust app library | 2,413 passed, 0 failed, 5 ignored |
| Full Node browser fixtures | 995 passed, 0 failed, 0 skipped; includes the corrected Window fixture |
| JavaScript syntax, whitespace and source/WASM identity | PASS |
| Actual browser QA | FAIL at strict network check; all 11 functional journey steps completed |
| Browser UX | Scoped PASS at 1200×1000; does not waive actual QA FAIL |

Commands:

```sh
cargo test -p beatkernel-bms-runtime --lib --no-default-features --features graphics --locked
node --experimental-vm-modules --test --test-concurrency=1 app/web/*.test.mjs
```

CLI logs are under `target/wf/bindings-qa-cli-12ad2-1/`: the first Cargo attempt
ended with exit 143 without a final summary, cause unconfirmed; its log is
retained. The exact warm-artifact rerun completed with exit 0. Node and other
completed checks exited 0. Earlier development fixture failures and corrections
remain recorded under `target/wf/bindings-development-12ad2/`; they are not
independent acceptance evidence. These evidence directories are ignored local
artifacts, not committed test results.

Actual production Chromium/SwiftShader completed row33 navigation/edit/refusal/
repair, parent-preserving Practice and Players returns, atomic Apply, DOM/reset
ownership and full-profile T0/T1/T2 roundtrip. Lane11 KeyA prepared the literal
pair `[17,19]`; old KeyZ produced no mapped event. Trusted KeyA Down/Up produced
one hit and zero misses. The genuine 6,251-byte `retained-keyA.bkr` capture
contains two Native lane17/key19 inputs (sequences 1/2) and 301 advances; its SHA256
is `53587a10712a63598824606ba9f7edc6f694f908ab17752fdab6c8e12e0a2f98`.
Playing those captured bytes after changing the live binding to KeyE again
produced one hit and zero misses, without live keyPairs/events. A separate
bounded unit fixture verifies the decoder's Native tag/backend/key layout; that
fixture is not actual playback proof.

The strict probe nevertheless fails `networkErrors.length === 0` at
`app/web/settings-policy.browser.mjs:514`: one audio-WASM GET reports HTTP 200
then `net::ERR_ABORTED`/CDP cancellation, while the server records 62,096 bytes.
Page and console errors are zero. The recurring cancellation's cause remains
unconfirmed; successful gameplay or a fully served body does not waive this
check. No diagnostic filtering, assertion relaxation or retry to obtain PASS
was performed. The original Settings-policy child remains unfinished too.

Browser evidence: `target/wf/bindings-qa-browser-12ad2-1/evidence.json` and
`target/wf/bindings-qa-browser-12ad2-1.log`; UX evidence:
`target/wf/bindings-ux-browser-12ad2-1/evidence.json`. Fresh screenshots were
inspected by both lenses. Their owned Chromium/Xvfb processes exited 0, HTTPS
servers closed and temporary profiles/cache/TLS were removed. Matching production
WASM SHA256 is `5f2ac6f3d87f8dc1772f0bf2119be74c3435f1709558de81a326b587cc7efe2e`.
Review/QA finals are substantive evidence; missing hook-owned receipts do not
authorize task close, and actual browser FAIL takes precedence.

UX backlog: the bitmap font renders unsupported hint punctuation as `?`; the
existing long acquisition form adds scroll travel between original controls and
retained canvas. Neither finding changes this scoped functional verdict.

## Known ceiling

Physical device/GPU latency, physical OS IME, screen-reader completeness,
responsive viewport coverage, loading speed and throughput are unverified.
This run tested 1200×1000 software-rendered Chromium only. Physical file-picker
cancellation is not claimed; deterministic fixtures cover cancellation and
hostile transaction branches. Strict browser acceptance requires resolving or
isolating the cancellation cause and a fresh unchanged-gate production probe
PASS; it cannot be inferred from CLI totals or functional journey completion.
