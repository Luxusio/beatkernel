# Retain historical comparison prefixes

The common archive and Step completion export retain bounded comparison
snapshots keyed by original player IDs. Browser completion export passes
Rust-owned saved-HUD snapshots through that cold path before
replay capture consumption. Existing v1/v2 paths remain byte compatible.

## Wire contract

Comparison-bearing archives use version 3. Their regular member entries retain
version-2 score tags. After all entries, the extension stores a little-endian
u32 table count, then each original u32 player ID and a byte optional-snapshot
tag. None is zero; Some is one. An attached table has one row per original member,
including explicit None rows; legacy archives have no table.

Some snapshots store u32 ghost count, then for each ghost: byte kind (own=0,
other=1), u32 UTF-8 label byte count and bytes, four u64 counters in hits/misses/
combo/max-combo order, and an optional signed i64 recorded-until timestamp.
A byte optional-network tag follows. Network state stores byte status
(waiting=0, connected=1, disconnected=2, stopped=3), optional progress tag, then
signed i64 song nanoseconds and four u64 counters in the same order. All integer
payloads are little endian. No floating-point conversion or rank inference occurs.

## Evidence and remaining integration

Both paired writers returned terminal stop reports. Seven archive and three
actual Step/Mixer fixture groups are authored. They cover independent literal
v3 bytes, legacy availability, original-ID projection, atomic attachment,
malformed/truncated/tagged data, aggregate envelope size and actual completion/
capture gates. An old v1 fixture only gains its absent-comparison initializer.
Attachment stages fallible copies, validates
whole-roster IDs, normalizes bounded labels to basenames and reuses the encoder
to enforce the entire 5 MiB limit before committing. Waiting network state cannot
contain progress. This cold size check allocates a temporary encoded buffer;
it does not run per frame. Scoped formatting and whitespace checks completed.
Four sequential compile-only checks exited zero: workspace/all-targets with
WebTransport, no-default-features WebTransport/all-targets, WASM browser/library
and WASM browser-audio/library. Host checks compile fixture children; WASM library
checks compile the actual browser export bindings without test children. Existing
unused-code warnings remain. No assertions were executed. Runtime checks remain deferred.
Native completion attachment and historical
comparison display remain subsequent work; current historical score/timing pages
do not display the new comparison metadata. Peer progress is self-reported and
saved replay summaries are operation prefixes. Neither provides trusted final
ranking or new live completion evidence. Full BMS player Goal remains active;
formal review and required browser/CLI/desktop QA are outstanding.
