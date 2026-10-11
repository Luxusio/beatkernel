# Browser record storage ownership

Status: Worker/Window implementation candidate, with scoped Node development
checks passing. Production probe integration and independent review/QA remain
pending. This does not complete the player or fix the recurring browser
audio-WASM cancellation.

The existing gameplay Worker owns same-origin IndexedDB record opening,
metadata listing, loading, saving and deletion. It also owns all replay/archive
body reads, private byte copies and File/Blob construction in these paths.
Window owns bounded metadata controls, input, permission gestures, correlated
operation tickets and the download object-URL bridge. It neither reads capture
bodies nor constructs replacement binary Files/Blobs for saving or downloading.
Reuse the existing database version and canonical formats; no automatic save,
upload, deletion, eviction or write retry is introduced.

## Immutable captures

The Worker validates genuine canonical capture bytes before creating a replay
File and optional whole-roster archive Blob. Terminal receipts carry these
immutable handles rather than raw byte arrays. Window admits only a matching
play owner, original roster, bounded handle sizes and valid completion/error
metadata. A terminal receipt alone cannot enable saving: original gameplay,
input and audio cleanup must join first. Natural completion and prefix labels
retain their existing meaning; labels cannot authenticate binary contents.

Each replay is nonempty and at most 64 MiB. Local captures retain their exact
original u32 player IDs/order, per-member allowance and aggregate 64 MiB limit.
An archive is nonempty and at most 5 MiB, belongs to the original whole roster,
and is associated with the selected original member. Never default to player
one. Validate standalone raw byte layout and buffer alias restrictions before
handle creation; handle reference identity alone does not prove byte uniqueness.
Handle construction failure reports bounded replay/archive errors without
fabricating successful capture or completion.
An otherwise valid solo replay handle is retained even when its terminal
receipt includes a bounded replay diagnostic: the final joined failure marks
the recording as a prefix while preserving download. Local per-member receipt
validation retains its existing stricter contradictory-data refusal.

Captured handles survive gameplay Worker termination for explicit download and
save retry. Download creates an object URL from the retained File, retaining
the existing complete/prefix filename and URL revocation. Loading creates a
File/Blob on Worker and selects it without starting audio. Historical/menu
preview reads and binary validation remain on Worker, preserving the prior
valid presentation until the current candidate is accepted. A selected replay
or opponent remains usable after its database entry is deleted or refresh fails.
Legacy records without an archive retain replay selection.

Menu preview permits one physical replay/archive acquisition at a time.
Competing previews acquire no body; navigation or cancellation cannot release
that slot before the original uncancelable read settles. Retired completion
cannot replace the current preview, and explicit retry is available after
settlement. This slot does not prevent unrelated gameplay.

## Record protocol

`records-request` carries a positive monotonic safe-integer operation `id` and
an action: `list`, `load`, `save` or `remove`. `load` and `remove` additionally
carry a distinct positive safe-integer `recordId` database key.
`save.value` contains bounded metadata (`name`, normalized `chartPath`,
`complete`, exact unsigned u64 bigint-or-null `hits`, `misses`, `combo`), a
`replayFile`, explicit nullable `archiveBlob`/`archivePlayer`, original bounded
unique `players` roster and selected original `player`. Validate and snapshot
all metadata, handle type/size and roster/member association before body reads.
Actual binary decoding remains Worker/Rust-owned.

`records-reply` carries the same `id` and `action`, with `result` and `error`.
Success has `error: null` and an action-specific result: list `{entries}`;
load `{metadata, replayFile, archiveBlob, archivePlayer}` with explicit nullable
archive fields; save `{metadata}` from a committed insertion; remove
`{removed: boolean}`. Failure has `result: null` and a nonempty diagnostic of
at most 4096 characters. Validate the entire reply before changing Window state;
only the current Worker, owner, operation/action and applicable menu ticket
may publish. Stale, duplicate or malformed replies cannot replace controls,
catalog or replay selection.

Records require CPU readiness and existing idle admission, respecting import,
preparation, settings, gameplay and room finalization. Optional record failures
do not make active gameplay fatal. Invalid IDs cause no effects. A newly
admitted physical operation consumes a monotonic ID floor; an active duplicate
is ignored, a used/stale ID cannot retry, and a competing busy request starts
no I/O and does not consume the admitted-operation floor. Retry is explicit
with a fresh ID.

## Cancellation and terminal ownership

`records-close{id}` fences the record epoch immediately. Its correlated
`records-closed{id,error}` reports a real joined cleanup or a bounded explicit
failure. A duplicate close joins or returns its own existing outcome without
starting another cleanup; stale close has no effects, and competing close
cannot replace the current closing owner. Hiding closes only the record owner,
preserving gameplay and immutable captures. Page reset/disposal fences pending
publication. Window shutdown retains its captured closing Worker/ticket for
cleanup even after advancing its ordinary page owner; previous record replies
remain ineligible to publish UI.

File/Blob reads and IndexedDB opens can outlive caller cancellation. Recheck
owner/epoch after every asynchronous body or chart read and before persistent
mutation. A retired operation retains physical capacity until underlying reads,
opening request and transactions terminate; cancellation/timeout must not
permit unbounded replacement operations or a fresh owner to bypass this slot.

Track opening request success/error, upgrade and schema-inspection transactions,
and store transaction complete/abort independently of already-rejected caller
promises. Close late opened connections. A cleanup wait is bounded to ten
seconds; missing terminal evidence reports failed cleanup/unknown outcome,
never successful release or physical termination. Fatal/dispose fences record
work at entry and preserves original genuine capture/room joins before terminal
cleanup reporting. Worker termination does not prove a write rolled back.

Save and delete retain atomic metadata/payload transactions. Only transaction
completion reports write success. Interrupted/timed-out writes have unknown
commit status: explicitly reopen and inspect, never automatically retry. A save
that committed before a failed refresh keeps that committed feedback. Existing
limits of 128 records and 256 MiB aggregate replay/archive storage remain.

## Verification

Use actual store source with manually controlled opening and transaction events
to test late opens, caller rejection versus delayed/missing native terminal
events, schema transactions, close/commit races and bounded cleanup failure.
Pure validators cover genuine immutable handles, original roster and size/error
boundaries. Actual Worker/Window fixtures exercise deferred reads, after-await
cancellation, busy/stale IDs, hidden/reopen and shutdown with closing-owner
tickets, retaining original capture/selection/retry assertions.

Node File cloning can differ from Chromium: retain explicit immutable endpoint
modeling and shared constructor/scheduler realms in VM fixtures. Actual File
clone/lifetime and exact capture bytes require production Chromium verification.
Worker-side observations inspect bounded genuine capture bodies without
replacing product replies or renderer ACKs. Keep exact original replay-header,
trusted key Down/Up and unmapped-key assertions and replay after binding changes.
Exercise actual solo/local save/load/history/delete/replay and cancellation;
observe zero affected Window database/body-copy work. Preserve all strict page,
console and network gates, including the unresolved audio request abort.

## Known ceiling

Browser-managed storage is best effort and may be removed by the browser/user;
explicit download remains available. File/Blob handles remove explicit large
JavaScript binary operations from Window but do not prove zero engine-internal
copies, an aggregate browser memory bound, physical input latency or superiority
over another player. Other browser engines and real device timing remain broader
Goal verification. The recurring audio-WASM cancellation is still unresolved.
