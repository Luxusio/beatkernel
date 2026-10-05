# Common room setup deadlines and browser phase waiting

RoomDeadline is portable fixed signed-nanosecond arithmetic: construction requires
nonnegative now, bounded unsigned timeout 1 ms..120 s and checked signed addition.
remaining_ns refuses negative time and expires at now >= deadline; retries never
renew the bound. It creates no timer, clock, IO or allocation. Native actor setup
checks use this same primitive, preserving the existing total elapsed
setup scope, clock checks, IO error precedence, late-receipt history and diagnostics.

RoomSetupWaitState owns the browser's existing staged scope: Admission initially
active, Lobby idle after actual admission, Prepared active with one new fixed
start-handshake deadline, Complete after genuine commitment. Lobby time is not
silently charged to either bound. Invalid/negative/regressing observations and
malformed/regressing phase flags refuse. Active deadlines are checked before
phase transitions or accepting commitment, so a delayed timer cannot make late
receipts timely success. Any refusal seals state; completed state is sticky.
State is not copied/reset to renew a wait. Pending returns whole remaining
nanoseconds rather than a periodic 1 ms poll; the host schedules this delay.
The timeout scopes are explicit lifecycle policies, not separate OS algorithms.

RoomClientDriver begin_setup validates once, and setup_step supplies only its
real admission/prepared/committed protocol evidence. Admission here requires
both a participant and the first actual room snapshot, preserving the previous
browser timer boundary rather than stopping on Admitted alone. No JS-supplied receipt flag
can advance phases. Close/failure/Leave prevents further setup success. A pure
read-only committed query does not consume the one-shot schedule. Browser binding
uses exact BigInt values: -2n idle, -1n complete, positive nanoseconds pending.
Errors retain phase information so timeout diagnostics identify setup/prepared.

The Worker initializes the Rust wait before connection IO and advances it after
actual receive/write credits, before publishing snapshot/start callbacks. One
pending timer invokes the Rust step instead of declaring timeout itself. Timer
rearming and receipt events keep the original fixed deadlines; idle/completion,
Leave/abort/close/failure clear pending timers. After Complete, the Worker skips
setup steps and their extra clock samples on gameplay IO. The Window gains no rendering or
new domain work. Actual transport completion timestamps and IDs remain unchanged.

## Known ceiling

Native drain arithmetic remains its existing actor implementation. Native total
setup and browser staged setup remain distinct existing scopes;
full configurable/unified room owner lifecycle is still unfinished. JS still owns
Promise/callback lifecycle, frame timers and cleanup. Tests are authored for later
execution. No browser/JS parsing, generated bindings, actual networking/timing,
physical audio sync, performance, formal review or QA acceptance is inferred from
source compilation. Full BMS player and SQLite-grade stability are unproven.

Seven pure policy groups, two genuine driver groups and five browser-adapter
groups are authored, unexecuted. All four compile-only configurations exited
zero after both writers stopped; see [the evidence scope](../changes/CHANGE__room-setup-wait.md).
