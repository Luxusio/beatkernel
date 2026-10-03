# Portable paired-stream ownership

AC-196 continues the same open player task after the browser Play integration.
The existing application crate gains a std-only room/participant registry as a
foundation for a future HTTP/3 adapter. No operating-system, socket, clock
acquisition, async runtime, protocol simulation, new dependency or crate is added.
This component does not itself accept a WebTransport connection.

`RoomPolicy::new` validates limits of 1–4096 rooms, 1–1024 key bytes and a positive
signed-nanosecond waiting TTL. Keys accept only ASCII letters, digits, hyphen
and underscore. `RoomRegistry::join` returns an owned waiting or paired outcome;
`release` and `expire` return exact closure tickets. Read-only room snapshots
and counts expose current membership without transport operations.

Each exact bounded ASCII room key admits one waiter and then one paired peer.
Configured capacity never evicts another room. Participants receive nonzero
monotonic u64 leases that are never reused, so a stale disconnect cannot remove
a newer same-key room. Releasing either paired member returns both precise
transport closure tickets; the caller owns and actually closes those streams.

Waiters have checked finite deadlines. Explicit expiration returns the exact
expired tickets, including the equality boundary. Attempting to pair with an
expired waiter returns an error without orphaning its resource; the caller must
process expiration first. A successful pair has no waiting deadline, so lengthy
active play does not inherit a lobby timeout. Caller timestamps must be
nonnegative and monotonic. Validation, counter/deadline overflow and capacity
checks precede membership mutation.

The current bilateral BKMP session still performs compatibility, readiness,
clock/start agreement and score exchange end to end. Room occupancy neither
authenticates peers nor creates ranked results. Supporting more than two network
participants needs a separate multi-party protocol design; local-player
collections remain independent. HTTP/3/TLS acceptance, transport resource joins
and native/browser interoperability remain unfinished.

Independent fixtures are authored for actual registry behavior, without mocked
protocols or transport execution. Assertions, browser/network/audio execution,
formal review and QA remain deferred. The persistent player Goal stays active
and the task remains open/PENDING; no PASS or acceptance claim is made.
After both production and independent fixture writers stopped, four genuine
compile-only terminal results were exit0: workspace all-targets, application
headless all-targets, WASM browser/lib and WASM browser-audio/lib, each --locked.
The two native paths compiled the seven actual registry fixture groups without
running assertions. WASM retained the three existing platform cadence dead-code
warnings. Scoped formatting and whitespace checks returned exit0. These checks
establish source compatibility, not executed registry behavior or server readiness.
