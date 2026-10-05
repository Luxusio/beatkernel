# Browser completed-result storage

The Worker exports one bounded whole-roster canonical archive from actual Rust
stepped completion before consuming any replay captures or freeing gameplay.
It preserves the original per-player capture header and actual gauge policy.
Cancellation, replay playback, a JavaScript completion choice, healthy gauge,
recorded prefix or cleanup acknowledgement cannot create a completed archive.
Missing later captures or invalid associations refuse the entire archive.
Export failure remains a separate recording diagnostic and does not erase
completion, prevent replay export, skip cleanup or turn a prefix into completion.

Solo/local Rust bindings provide the export; encoding is cold finalization work
on the Worker, with no per-note allocation, virtual dispatch or Window renderer.
The final Worker message transfers one standalone Uint8Array at most 5 MiB,
with existing owner/play identity and member replay transfer rules. The Window
retains these opaque bytes with captured recordings and the original player ID.
It does not decode the format or infer completion from replayComplete or scores.
Finite message/roster/buffer validation lives in the existing pure completed
results model so it can be tested without DOM, renderer or IndexedDB. Window
owner-generation checks and lifecycle cleanup remain in the outer adapter.

Saving to IndexedDB includes the replay and its optional whole-roster result
archive in the existing recording transaction. Validate bounded standalone
buffers and original u32 player IDs before private snapshots or mutation. Count
both payload sizes toward the existing 256 MiB library budget, with no silent
eviction. The archive may be shared by local recordings in memory but each
persisted recording owns its bounded copy and capacity charge. Atomicity refers
to the existing IndexedDB transaction, not power-loss or crash durability.

Existing metadata/recordings without archives remain compatible. New archive
metadata/payload associations must agree exactly when loading; inconsistent
lengths, missing payload, oversized bytes or invalid IDs refuse the stored entry.
Loaded bytes remain untrusted historical data and are decoded and exactly
associated by Worker-owned Rust before result presentation; see
[the loaded historical record contract](REQ__browser-historical-record.md).
No storage metadata or file authenticates live play. Actual Step completion
exports now include exact score/timing details under
[the version-2 contract](REQ__archived-score-details.md); opaque storage needs
no format parsing. Comparison archival and actual browser acceptance remain
unfinished.

Independent deferred fixtures cover actual stepped solo/cohort completion and
refusal, byte/header/gauge identities, malformed late members, Worker transfer
and cleanup ordering with retained proof/errors, and injected IndexedDB commit,
abort, corruption, compatibility and capacity cases. Tests, JavaScript parsing,
generated bindings, real browser/filesystem/device execution and formal QA remain
deferred under the user's instruction. Scoped formatting and the exact four
compile-only checks follow both writers' terminal stop; they cannot establish
JavaScript/browser behavior or zero-overhead performance.
