# Explicit browser record library

The browser host offers explicit save, metadata refresh, saved-replay selection
and selected-record deletion. RecordsStore uses same-origin IndexedDB with
separate bounded metadata and canonical byte stores. Listing acquires no replay
payload. Save checks the aggregate limit and inserts both records atomically;
delete removes both atomically. Success requires the transaction complete event,
not an individual request result. Initial limits are 128 recordings, 64 MiB per
file and 256 MiB total encoded bytes; records are never automatically evicted.

Saving uses the actual capture after audio/game cleanup joins, its original
chart path and readable score. Loading selects a File for the existing replay
path without starting audio; the next Play replay button preserves audio resume
in its user gesture. Stored complete/prefix labels are display metadata, not
canonical authentication or chart-match proof. Busy ownership, page-generation
guards, aborted transactions, late connection cleanup and version-change
fencing prevent old library operations from replacing a new page's selection.
Failures retain the current capture, replay and download paths.

Signed song-time readout now formats negative preroll and i64 minimum with one
sign and exact absolute fractional nanoseconds. Preview admission remains
nonnegative. Behavior is specified in
[the browser requirement](../kernel/REQ__bms-browser.md).

Known ceiling: browser storage is best effort and may be removed by the
browser/user; explicit download remains available. JavaScript/IndexedDB/browser
execution, generated bindings, audio/GPU/hardware behavior, authored fixture
assertions and formal review/QA remain deferred. Browser competition against
stored records and WebTransport integration are still unfinished Goal work.

API semantics: [transaction complete](https://developer.mozilla.org/en-US/docs/Web/API/IDBTransaction/complete_event),
[blocked opens](https://developer.mozilla.org/en-US/docs/Web/API/IDBOpenDBRequest/blocked_event),
[version changes](https://developer.mozilla.org/en-US/docs/Web/API/IDBDatabase/versionchange_event)
and [browser storage limits](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria).
