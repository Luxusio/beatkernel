# Song catalog search

Selection now searches cached lowercase title/artist text with whitespace-separated
substring tokens. Changed queries rebuild original catalog indices once and keep
the current chart when still matched; empty results cannot launch the previously
selected hidden chart or open its Records panel. Fifteen retained rows use original
hit IDs, and unchanged redraws reuse the projection. F3/click focuses the bounded
editor, with text and IME commit input. Enter exits editing; Escape clears it.
Settings Back and Play return retain the query while route changes clear focus.

Known ceiling: cached search text adds memory proportional to catalog text, and
matching is linear on query edits. Unicode lowercasing is not full case folding,
locale collation, accent removal or fuzzy search. IME integration is authored but
not executed. Live coordinated pause/loops, browser/full widget host and full
native/GUI acceptance remain unfinished. All tests/product/GUI/native/GPU/shader/
device/file/network/bench and independent review/security/QA remain user-deferred.
Full Goal/task remain open without acceptance PASS.

Source validation: Linux, Windows GNU and macOS app all-targets, headless
all-targets and WASM graphics library checks succeeded. Final affected desktop
checks after Start-button focus correction also succeeded. Scoped Rust 2024
formatting and diff whitespace checks succeeded. Existing macOS block future
compatibility and WASM cadence warnings remain. Pure fixtures were authored
and compiled only; no tests or product were executed.
