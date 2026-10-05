# Portable room client driver used by WASM

The existing BrowserRoomClient delegates decoder, session, revision, pending peer
token and failure/lifetime state to the common RoomClientDriver. Its WASM methods
retain existing bounded arrays and exact integer conversions. Actual async write
completion IDs and timestamps still come from the Worker transport; obtaining
an outbound frame supplies no completion credit. Independent host fixtures can
now exercise this same binding-owned state policy without JS values or browsers.

## Known ceiling

JS result conversion and Worker async orchestration remain adapters. This change
does not integrate RoomNetworkActor/RoomCompetition with the browser owner or
prove real WebTransport, physical timing or measured performance. Seven pure
fixture groups are authored but unexecuted. The seventh uses real common
registry/clock/start and progress-relay operations for one-shot schedule and
held/consumed/colliding peer-token paths, without seeding driver state. Final
ACK/drain integration and allocation fault injection remain coverage gaps in
these new fixtures.

After both writers stopped, scoped formatting and four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, no-default-features
WebTransport/all-targets, WASM browser/library and WASM browser-audio/library.
The initial workspace check exposed the obsolete WASM-only buffered-byte query
gate. Its owning source lane removed that gate without changing the method body
or visibility, then the repeated workspace check succeeded. Remaining warnings
concern existing unused code. Host checks compile the pure fixtures; WASM library
checks compile actual binding/driver calls but do not compile or execute fixtures.
Tests, runtime, formal review and QA remain deferred.
