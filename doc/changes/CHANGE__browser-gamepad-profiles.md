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
capture/replay or latency acceptance. Axis bindings do not prove
ordinary press-chart support; local multiplayer assignment is separate work.
Polling and committed-frontier late-input refusal remain observable limits.

Follow-up: align setup coverage with the existing canonical event type.
Touched controls (type 3) already emit Button Down/Up in their own native
namespace, so they now satisfy ordinary press-lane setup coverage beside type 0.
The common binding and interaction evaluator accepts those real Button events.
Stick/analog-button axes remain excluded. No threshold, keyboard translation,
timestamp change or separate interaction evaluator is added. Deferred regression
fixtures cover touched-only setup and Worker ingestion: one new profile group
and one new play Worker group, bringing source totals to nine and 62. They
check initial false/noop/Down/Up, pressed/touched namespace separation, full-width
source and original timing, and numeric/file-profile admission into input_blob.
Prior axis-only refusal remains. Both authors stopped before integration;
scoped whitespace inspection was clean. Tests and browser/device execution
remain pending; no parsers, Cargo checks, formal review or QA were run.
