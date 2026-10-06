# Shared caller-driven room network actor

RoomNetworkActor<S> belongs to an unconditional platform-common module. S
implements RoomNetworkStream: Read + Write with explicit idle(Duration) and
finish(Duration) operations. Streams must provide nonblocking or bounded IO;
the actor acquires no thread, timer, socket, player or physical output. The
native NativeRoomStream name remains an alias to this same port, and its actual
WebTransport implementation remains outside the actor.

The actor retains the existing RoomPlayIo owner, validated RoomNetworkOptions,
room snapshot, exact signed observation chronology and one fixed drain deadline.
new validates options before operations on the provided stream. command, drive,
snapshot, finished and finish retain original semantics, messages and ownership.
drive uses caller-provided time on every existing observation and IO bracket;
negative/regressing observations and checked deadline overflow refuse exactly
as before. Setup and drain deadlines are never renewed by polling or queue pressure.

Retained roster revision increments only on metadata changes. Roster/prefix Arc
sharing, frozen peer ordering and original sequence/identity fields remain exact.
Historical receipt bits accumulate only from the actual common IO owner. A later
operation failure keeps accepted earlier data but cannot make late completion
timely. finish preserves original operation error before refresh error, then
stops the common owner and attempts injected stream cleanup once. Cleanup error,
receipts, leave-written status and caller cancellation stay independent fields.
Retained participant/roster/peer/receipt refresh now uses
[the shared projection](REQ__room-snapshot-projection.md) also used by browser
snapshot export. Dirty notification preserves accepted earlier mutations when
a later projection fails; genuine schedule consumption remains actor-owned.

take_changed_snapshot returns the retained snapshot only when dirty and clears
that notification once; idle delegates explicitly to the stream port, and
leave_written reads the actual common owner. The native worker uses these
accessors rather than private actor fields. RoomPlayIo's crate-private mutable
stream accessor becomes platform-common without changing its behavior. The
native worker still owns acquisition, absolute Instant origin, bounded command
queue/reply credits, shared locking, thread join and actual endpoint cleanup.

Existing native actor fixtures continue to target the same extracted actor.
Independent new fixtures use only common protocol owners and memory streams to
cover preflight, command/clock refusal, fixed deadline crossing, original IO and
cleanup diagnostics plus snapshot ownership. Fixture authorship and host/WASM
compilation supply no execution or real network/timing evidence.

## Known ceiling

Browser stream/actor integration and native worker scheduling remain work.
Read/Write and cleanup effects are explicit injected ports; this does not prove
adapter nonblocking behavior, actual peer delivery, crash safety, measured
zero-cost dispatch, hardware latency or SQLite-grade reliability. Tests, runtime,
formal review and QA remain deferred.
