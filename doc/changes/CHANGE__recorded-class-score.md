# Recorded class score consumers

Historical archive entries can derive checked BMS class counts and EX points from
explicit recorded policy and validated stored grade counts. Unknown grades,
overflow and classified gauge mismatch refuse at archive validation. The
projection uses existing sorted storage without rebuilding a map or changing
archive formats. Unclassified and scoreless archives expose no guessed BMS score.
The logical replay inspector now accepts complete section/gauge/class metadata,
prints actual prefix scores and refuses diagnostic seeks past finite endpoints.
It reads no PCM/native devices and persists no completion.

Verification: full library tests passed 1,639 with two ignored; all seven binary
suites passed 367. Independent QA passed the four new archive groups, seven
inspector tests, existing archive/judgment regressions, workspace all-target and
WASM/browser checks, and 15 actual inspector command cases. PGREAT2 reports EX4;
GREAT1/POOR1 reports EX1; cursor zero and legacy unclassified behavior remain
explicit. Invalid metadata/source and finite-endpoint overruns refuse. Independent
DEEP code and security reviews passed. The initial all-bin/workspace compile
failure from fixture placement is retained in local logs; fresh review and QA
after relocation passed. Evidence is under `target/wf/recorded-class-score-*`.

CLI test helpers belong under `src/bin/fixtures/`, referenced by `#[cfg(test)]`
modules. Cargo automatically discovers top-level `.rs` files in `src/bin/` as
binaries; focused single-bin tests do not catch that placement error. Verify the
target list and all-bin/all-target commands when adding a binary fixture.

## Known ceiling

Native recording, live/classified HUD and retained record selection still need
class-policy propagation. Archive projection validates historical data, not
physical completion or authenticity. Generic combo rules, historical timing,
empty POOR/mine scoring and custom networking remain separate work in the full
active Goal.
