# Browser loaded historical result presentation

Using a stored replay may send its original replay bytes, optional whole result
archive and explicit original player ID to the initialized idle Worker. Window
owns IndexedDB reads, UI controls and operation cancellation only. It does not
decode archive gameplay fields, calculate outcomes, render result geometry or
schedule a rendering loop. Loading a historical result never creates live
CompletedPlayResult evidence or CompletedResults metadata with proof=true.

Shared pure Rust policy completely decodes the bounded replay and archive and
uses exact canonical ReplayHeader/original-player association. Historical
presentation displays the original player, full/practice scope, exact signed
nanosecond extent, stored outcome and exact gauge units, with explicit stored
historical provenance. Version-2 stored score counts and timing use
[the exact detail contract](REQ__archived-score-details.md). Version-1 omission
remains unavailable; the display cannot invent these statistics.
The shared from_record presentation builder marks associated version-1 score
metadata as STORED SCORE UNAVAILABLE. Native Records reuses this builder under
[its detail contract](REQ__native-record-details.md); browser decoding and
association remain Worker-owned.
Repeated drawing uses cached geometry rather than
reformatting strings. This path owns no gameplay, network, clock or audio state.

Absent legacy archive data leaves replay selection usable. Corrupt, oversized,
ambiguous or mismatched archive data refuses historical display and reports a
bounded diagnostic while keeping the selected replay available. Window/Worker
responses use a positive monotonic operation ID, and stale async reads cannot
replace a newer selection or resume after cancellation. New play, chart/library
selection, preview seek, imported replay selection, reset and fatal owner disposal
invalidate and release historical presentation. It may render only while idle;
it cannot replace an active gameplay or joined live Results screen.

## Historical checkpoint — superseded as of 2026-10-10

The following paragraphs describe the earlier compile-only increment. Their
verification deferral is no longer active; current development checks and
remaining independent QA are recorded in the later dated section.

Independent deferred pure Rust and actual Worker-message fixtures cover original
IDs, long extents, malformed later archive rows, legacy absence, idle admission,
stale/cancelled reads and binding release. Tests, JS parsers, generated WASM,
browser/device/GPU applications and formal review/QA remain deferred by the
standing instruction. After both paired writers stop, scoped formatting and the
exact four compile-only checks provide compilation evidence only.

Implementation now supplies a graphics-gated pure HistoricalRecordPresentation,
a separate BrowserHistoricalRecord binding and Worker-owned cached drawing.
Window validates archive byte layout/size and original u32 member ID,
transfers that opaque buffer, and waits at most ten seconds for its matching
operation response. Cancellation and timeout send a newer clear request. Legacy
records without archives select the replay and clear historical display normally.
Showing a stored result needs no prepared chart. Active gameplay and retained
joined live Results refuse historical replacement with a diagnostic. Preparing
another chart or beginning another accepted play releases prior history.
Window page controls validate bounded metadata and correlate one pending request
through [the browser grade-page contract](REQ__browser-grade-pages.md), without
decoding statistics or owning canvas rendering.

## Explicit stored-record navigation from solo/local Results

After solo or local play has finished and its owner has retired, selecting a
stored record is an explicit navigation request. A valid current historical
candidate may replace that completed Results display without chart reprepare.
Retained joined-room Results still refuse historical replacement; active play
and room finalization remain unavailable for this operation.

Acquire and validate the candidate before retiring the previous valid
historical or eligible completed Results owner. Invalid bytes, read failure,
constructor failure, unavailable metadata and stale acquisition retain the
prior valid presentation. A successful current commit releases old bindings
exactly once and prevents late Results page replies from restoring obsolete
controls. Explicit cancellation, new play and owner disposal invalidate pending
acquisition; an old cancellation cannot release a newer accepted owner.

Candidate acquisition replies with validated metadata while the prior owner
remains usable. Window accepts only its matching current response and sends
`historical-record-accept` before retiring its prior controls. Worker commits
only the latest still-eligible candidate on that acceptance. If Window times
out before processing the candidate reply, cancellation releases the candidate
and preserves the prior owner; a late reply must not send acceptance. Test this
ordering against the actual Worker and subsequent prior-owner page navigation.

A rendering failure belongs to its correlated visual owner. Failure of the old
historical presentation releases that presentation and reports its original ID;
it must not invalidate a newer acquisition or staged candidate. Acceptance and
page navigation for the new candidate remain valid. Late errors from a retired
visual cannot release the current replacement. Explicit reset, new play and
disposal still release pending candidates along with their owned presentation.

Window handles only correlated metadata and cancellation. Historical decoding,
archive/player association, cached geometry and rendering remain Worker-owned.
Verify connected completion -> save/use -> historical presentation, failure
retention, deferred-read ordering and joined-room refusal through real handlers
and the current generated browser binding.

## Development verification — 2026-10-10

The connected ownership correction passed thirteen focused handler tests.
Related historical/results/Window/audio-model regression passed 238 tests with
zero failures. Independent discovery then identified an old-history render
failure that invalidated the new candidate; after the focused owner correction,
the entire historical Worker file passed 22 tests, including two new staged and
deferred-acquisition render-error interleavings. Each proves real acceptance,
drawing and page responses through the Worker harness. The unchanged unrelated
green tests were not repeated for that focused correction.

These are development handler fixtures, not actual browser GPU acceptance.
Fresh full-sweep source review and independent CLI/browser QA remain required.

## Historical compile-only evidence — superseded as of 2026-10-10

Both paired writers returned terminal `Writes STOPPED` before formatting and
compilation. Seven deferred groups were authored: three pure Rust byte/geometry
groups and four actual Worker-message groups. Scoped formatting, diff whitespace
checking and all four compile-only checks exited zero. No assertions, JavaScript
parsing, generated binding execution, real Window lifecycle, browser/IndexedDB/GPU
acceptance, allocation measurements, formal review or QA were performed.
