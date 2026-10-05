# Browser room drain through the common final policy

The split-operation RoomClientDriver retains final-prefix and drain admissions
only after its actual protocol operation succeeds. Neither flag represents a
write completion, ACK, local receipt completion or coordinated room completion.
A drain wait observes these genuine admissions through RoomFinalWaitState;
already-published final progress transfers admission evidence without reissuing
the publication. Before that evidence exists, progress remains pending. Drain
capacity remains pending until local completion permits the direct protocol
request. Only successful request_drain supplies drain-admission evidence.

begin_drain validates a nonnegative signed room-domain now, a 1 ms..120 s unsigned
timeout and checked fixed deadlines in signed network and unsigned control time
before changing state. Invalid or repeated setup refuses without renewing an
existing deadline. drain_step uses the supplied room elapsed observation for
both adapters, preserves clock regressions and all common receipt/terminal gates,
and never parks or moves transport bytes. The policy state's terminal seal is
retained across calls. Leave/cancel/failure cannot become drain success. Closing
drops wait state and protocol owner together; old lifetime calls cannot rejoin.

The WASM binding exposes begin_drain, drain_wait_step and drain_requested with
exact BigInt times. Step returns -1n for completion or nonnegative nanoseconds
for scheduling; original protocol errors retain their diagnostics and timeouts
are explicitly coded. The Worker room owner begins once, repeatedly invokes the
Rust step on receipt events or a bounded pending timer, and resolves only on
Rust completion plus its validated real receipt snapshot. It clears pending
timers on leave/close/failure, retains one drain Promise and sends transport
write IDs/completion observations only after actual async write completion.
JavaScript retains asynchronous lifecycle/cleanup, not duplicate drain policy.
A settled drain Promise is never stepped again, including a recoverable local
state refusal that leaves the transport owner alive; later receipts retain
history without restarting the sealed wait or admitting another command.

## Known ceiling

Pure and adapter fixtures are authored for later execution. Browser generation,
JS parsing/execution, real WebTransport/ACK/timing and performance acceptance,
formal review and QA remain deferred. Full room owner/controller orchestration
and full BMS player requirements are unfinished; compilation alone is not
playable-browser or SQLite-grade reliability evidence.

The four compile-only configurations exited zero after both paired writers
stopped. Three pure driver groups and four browser-adapter groups were added,
unexecuted; see [the evidence scope](../changes/CHANGE__room-driver-drain.md).
