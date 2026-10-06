# Browser stored grade paging

Window exposes Previous/Next stored-record-detail controls and a page-status caption for
the currently selected historical record. It sends only bounded page requests;
it does not decode archive statistics, calculate grades, format grade counts,
draw canvas geometry or own a rendering loop. Worker owns BrowserHistoricalRecord
and invokes its existing Rust grade-page API, using cached four-row presentation.

Use the existing shared host_model module for envelope validation and a pure
HistoricalGradePager. Public API: bind(id,page,pages), snapshot(), request(page,rpcId),
accept(reply), cancel() and clear(). Snapshot is null or {id,page,pages,pending}.
Page metadata validates integers, 1..1033 pages and 0<=page<pages before mutation.
The same bridge now covers grade pages followed by stored comparison pages under
[historical comparison details](REQ__historical-comparison-pages.md). Existing
grade-page API/command names remain compatible; Window does not inspect page content.
Selection IDs and RPC IDs are positive safe integers; accepted request IDs increase.
One pending request is allowed. Same-page/busy/unselected actions produce no command.
Requests contain {kind:historical-record-page,id,rpcId,page}. Matched successful
receipts must return the requested page and original page count. Stale/duplicate
receipts do nothing. Malformed matched receipts refuse before state mutation.
Matched error receipts release pending state but retain the last confirmed page.

Initial historical-record-result adds gradePage and gradePages for available
bindings; unavailable results use null/0. Worker reads and validates the genuine
Rust getters before admitting a displayed record. Page receipts use
historical-record-page-result with id,rpcId,gradePage,gradePages,error. Success
has validated metadata and null error; refusal has null/0 and a bounded diagnostic.
No new gameplay or completion evidence is created.

Worker admits page commands only for the exact live historical selection while
initialized and idle. Check active play, import/settings/selection, retained live
Results, invalid IDs, stale/duplicate RPC and page bounds before calling the setter.
Failed admissions do not advance the successful RPC frontier. Same-page accepted
requests acknowledge current metadata without calling the setter or scheduling
another draw. Successful changes validate the setter's resulting page/count and
schedule Worker redraw. Recoverable setter refusal keeps the same binding and
confirmed page; invalid/unexpected changed metadata disposes the display and
reports unavailability rather than publishing a mismatched page. Schedule
a Worker redraw after invalid-state disposal to restore ordinary preview geometry.
No synchronous page handler yields between its guard and mutation.

Window paging uses the current worker-owner identity and historical selection,
separate from the completed library-load operation. Disable controls while a page
receipt is pending or another owner operation is busy, and hide them when there
is no multipage historical selection. Only matching receipts update the caption.
Keep one 10-second page deadline. Timeout/post failure/malformed matched receipt
clears historical display through the existing clear route, so an unknown applied
page cannot leave stale controls; the selected replay remains usable. A failed
page post shows a bounded diagnostic as well as clearing its uncertain display.
Cancellation,
owner reset, saved-record selection change, new play/import/seek and disposal clear
pending state/timers. Abort pending library selection work when that selection
changes; an unabortable late read must not replace a newer selection. Late timers
and responses cannot clear or modify a replacement record.

## Evidence and known ceiling

Author independent pure pager boundary/order/atomicity cases and actual Worker
and main.js lifecycle fixtures with injected Rust binding/DOM/Worker/storage.
Reuse existing harnesses and preserve earlier assertions, adding only scripted
grade getters and new successful-response metadata where required. These scripts
do not prove generated Rust binding execution, real browser storage or GPU output.
JavaScript parsing, Node/test execution, apps, formal review, QA, verify and close
remain user-deferred. Text inspection and whitespace checks are available; earlier
Rust compile evidence applies to the unchanged grade adapter, not this JS routing.
The full BMS player Goal remains active, with platform acceptance, measurements
and room-wide historical metadata still unfinished. Common/native/browser comparison
archival and historical detail display are source-integrated; execution remains unproven.
