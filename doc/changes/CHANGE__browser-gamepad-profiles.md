# Optional browser Gamepad profiles

Connect explicit nonstandard Gamepad profiles through the actual live page and
Worker pipeline. Window retains optional profile selection, validates only file
metadata and forwards the original File and acquired device descriptors. Clear
restores automatic standard mapping. Worker reads the bounded file exactly
once, parses strict UTF-8 JSON version 1, validates every profile, matches
actual products/mappings/counts and creates exact-source canonical control
bindings. Ambiguous or unmatched explicit profiles fail clearly. Device IDs
retain all 64 bits; analog controls remain typed axes.

Snapshot descriptors and file size before asynchronous work, then guard current
ownership after reads. Verify actual admitted sources against the live owner
before activation. Replay ignores live profile drafts and performs no live
acquisition. Profile/clear UI updates are event-driven and locked during work;
selection survives stop and invalid replacement.

Six deferred fixture groups cover strict profile decoding and source matching,
Worker read-once/snapshot/cancellation behavior, and Window selection locking,
retention, clear, replay isolation and source admission. Source totals are eight
Gamepad profile groups, 61 play Worker groups and 69 play host groups; the 14
preview Worker groups remain unchanged. These are authored source counts, not
executed results. Scoped git whitespace inspection completed without diagnostics.
No tests, JavaScript parser, browser/device/audio run, Cargo build, formal review
or QA was executed for this JavaScript-only increment. Runtime acceptance and
required browser QA remain pending under the user's execution deferral.

Known ceiling: Source integration does not prove actual browser/device/audio,
capture/replay or latency acceptance. Axis/touched bindings do not prove
ordinary press-chart support; local multiplayer assignment is separate work.
Polling and committed-frontier late-input refusal remain observable limits.
