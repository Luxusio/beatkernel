# Portable split-operation room client driver

RoomClientDriver owns the common RoomPlayClient, incremental RoomFrameDecoder,
latched RoomPlayError, metadata revision and one pending accepted peer token.
It belongs to an unconditional module without JS values, browser APIs, timers,
transport acquisition, native owners or physical output. BrowserRoomClient uses
this actual driver for state operations; WASM bindings only convert bounded
input/output values and expose existing methods.
RoomFrameDecoder's crate-private buffered-byte count is available on every
platform. Its length-only implementation and visibility remain unchanged;
the obsolete WASM-only gate is removed for this common driver.

Driver construction accepts borrowed identity/player values and the existing
StartPolicy/preroll. Local request-state refusals retain the same recoverable
classification. Malformed evidence and fatal operations latch the first failure.
Receive validates capture/observation chronology and current decoder prefix
bounds before accepting bytes. Only complete accepted Admitted/Snapshot frames
advance metadata revision, checked before acceptance. PeerProgress keeps one
pending participant token until the binding successfully materializes its latest
accepted prefix; collision refuses rather than dropping evidence silently.

next_write returns the original optional OutboundFrame allocation and ID;
written accepts only the caller's actual completed write ID and observation
times. Buffered bytes or frame admission never establish write completion or
ACK. take_start consumes only the genuine common schedule. Historical local
final/ACK bits remain queryable after latched failure while progress/drain
completion remains guarded by facade health. close idempotently stops/releases
the session, decoder and peer token while preserving an earlier failure and
metadata revision.
Scalar due and cadence-aware publication now follow
[shared room publication timing](REQ__room-publication-cadence.md). The actual
browser Owner samples its elapsed clock and Worker consults the due hint before
building progress words. Queue admission remains separate from write completion.

Browser exports keep their signatures, BigInt identity/time/counter domains,
bounded words decoding, snapshot/prefix serialization and code="state" tagging.
Failure to create a JS result or assign an error property still closes the
driver as before. Pending peer materialization clears its token only after
successful JS conversion. The Worker retains asynchronous channel ownership,
actual completion observations and cleanup; the Window gains no rendering or
network loop.

Independent host fixtures exercise this actual driver with common protocol
frames, fragmentation, local/fatal refusal, full-width identity/time values,
write completion authority, metadata revision, pending peer ownership and close.
Host compile checks now include those pure fixtures. WASM library checks compile
the real binding/driver call path; assertions and browser execution remain deferred.

## Known ceiling

This driver preserves the asynchronous split-operation contract; it is not a
browser stream adapter for RoomNetworkActor or an integration of RoomCompetition
with the existing Worker UI. Async orchestration, browser result conversion,
actual WebTransport, physical timing and platform/performance acceptance remain
unverified. No new per-note queue, lock, dynamic dispatch or speculative write
receipt is introduced. Tests, formal review and QA remain deferred.
