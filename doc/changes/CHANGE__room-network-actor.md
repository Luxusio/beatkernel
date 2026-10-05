# Shared room network actor

The native worker delegates its caller-driven room protocol state to the common
`RoomNetworkActor`. Streams implement explicit read/write, idle and cleanup
operations, while the worker retains real clock acquisition, queue credits,
locking, cancellation and thread join. The same actor retains original roster
and prefix snapshots, fixed deadlines and historical receipt/error fields; native
stream naming remains a compatibility alias.

## Known ceiling

Browser adapter integration, native worker scheduling, actual nonblocking IO,
peer delivery, platform timing and measured performance remain unfinished or
unverified. Six independent memory-stream fixture groups are authored but
unexecuted. After both writers stopped, scoped formatting and four sequential
compile-only checks exited zero: workspace/all-targets with WebTransport,
no-default-features WebTransport/all-targets, WASM browser/library and WASM
browser-audio/library. Existing native actor fixtures remain compiled in both
host checks. WASM library checks compile the actual common actor but not its
fixtures. Only existing unused-code warnings remain. Test execution, formal
review and QA remain deferred.
The new bounded fixtures do not cover committed drain arithmetic overflow or
refresh allocation failure injection. Existing protocol and native actor fixture
authorship remains separate from executed evidence.
