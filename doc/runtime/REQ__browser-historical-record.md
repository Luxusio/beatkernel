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
historical provenance. It cannot invent archived score/timing statistics omitted
from archive version 1. Repeated drawing uses cached geometry rather than
reformatting strings. This path owns no gameplay, network, clock or audio state.

Absent legacy archive data leaves replay selection usable. Corrupt, oversized,
ambiguous or mismatched archive data refuses historical display and reports a
bounded diagnostic while keeping the selected replay available. Window/Worker
responses use a positive monotonic operation ID, and stale async reads cannot
replace a newer selection or resume after cancellation. New play, chart/library
selection, preview seek, imported replay selection, reset and fatal owner disposal
invalidate and release historical presentation. It may render only while idle;
it cannot replace an active gameplay or joined live Results screen.

Independent deferred pure Rust and actual Worker-message fixtures cover original
IDs, long extents, malformed later archive rows, legacy absence, idle admission,
stale/cancelled reads and binding release. Tests, JS parsers, generated WASM,
browser/device/GPU applications and formal review/QA remain deferred by the
standing instruction. After both paired writers stop, scoped formatting and the
exact four compile-only checks provide compilation evidence only.

Implementation now supplies a graphics-gated pure HistoricalRecordPresentation,
a separate BrowserHistoricalRecord binding and Worker-owned cached drawing.
Window validates only archive byte layout/size and original u32 member ID,
transfers that opaque buffer, and waits at most ten seconds for its matching
operation response. Cancellation and timeout send a newer clear request. Legacy
records without archives select the replay and clear historical display normally.
Showing a stored result needs no prepared chart. Active gameplay and retained
joined live Results refuse historical replacement with a diagnostic. Preparing
another chart or beginning another accepted play releases prior history.

Both paired writers returned terminal `Writes STOPPED` before formatting and
compilation. Seven deferred groups were authored: three pure Rust byte/geometry
groups and four actual Worker-message groups. Scoped formatting, diff whitespace
checking and all four compile-only checks exited zero. No assertions, JavaScript
parsing, generated binding execution, real Window lifecycle, browser/IndexedDB/GPU
acceptance, allocation measurements, formal review or QA were performed.
