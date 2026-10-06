# Validate image plans and selected-file budgets before acquisition

Image preparation now builds one pure ImagePlan before filesystem root
canonicalization. The same plan constructor validates scoped selected-file
preparation; the validated plan is consumed by loading without rebuilding its
reference/source sets. Invalid crop IDs/rectangles, visual counts and canvas
headers/extents/bytes therefore fail before filesystem/source access. Existing
alias sharing, source retention, layer keying and decoded-byte accounting remain.

MemoryFiles construction now rejects a per-file maximum larger than the total
maximum, while preserving existing positive/representability checks and defaults.
This prevents accepting an incoherent budget before any files are admitted.

Before the fix, asset-source tests were 5 passed / 1 failed and preparation tests
24 passed / 3 failed. Afterward all six asset-source tests passed and preparation
reported 26 passed / 1 existing tagged-MP3 failure. The original assertions were
not changed. Full library execution reports 1466 passed / 97 failed, all remaining
failures present in the prior baseline. Workspace all-targets WebTransport,
WASM browser compilation and whitespace checks exited zero with only existing
unused-code warnings; no actual
native pixels, audio/acoustic or GPU acceptance is inferred. Review/QA and full
Goal completion remain outstanding.
