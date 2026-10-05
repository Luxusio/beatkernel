# Competition progress network policy port

Solo and local-group competition progress observation consume a shared generic
network port. The port supplies an owned notice iterator, readiness/start state,
borrowed whole-member publication, room observation and nonblocking stop request,
with an associated opaque error. Shared policy imports no concrete native socket,
thread, clock, UI, filesystem, endpoint owner or diagnostic output implementation.
Remote notices remain comparison data and never enter a local judge.

Shared notice policy drains the current notice batch, accepts Connected/Ready
only while the comparison is active, and retains the first original disconnect
error. Terminal/disconnected/stopped state cannot be revived by later readiness
notices. A repeated Connected notice cannot downgrade already observed Ready;
Stopped remains stopped even when a later disconnect error is returned.
Disconnect takes precedence over readiness in the same active batch. A later
valid local score report remains usable after comparison failure. Only the
adapter translates transport events and performs socket/thread effects.

Shared publication policy receives borrowed original MemberProgress values and
explicit caller policy inputs. Bilateral publication requires allowed state,
actual readiness, caller-required committed start and a due cadence. Successful
admission alone permits the caller to update its last-publication marker. A
refused effect retains the original associated error; it cannot advance cadence
or reject an already committed local judge result. Room observation preserves
its separate controller policy and borrowed inputs; it must not be classified as
bilateral publication or imply completion.

Native solo/group observation paths compose this port through the same adapter.
Existing solo song-time and group control-time cadence and start requirements
stay distinct and unchanged. The adapter maps notices without allocating a second
notice vector, and copies member data only at the actual owned publication
boundary. Static generics add no dynamic dispatch, lock or allocation requirement
to the shared abstraction. Socket workers remain owned until cleanup; observation
can request stop but cannot join, create an endpoint or fabricate completion.

Independent deferred fixtures inject iterator notices and errors without real
network/time/UI, covering event ordering, terminal monotonicity, readiness/start/
cadence gates, exact sparse u32 members, refusal and room routing. Existing
native competition fixtures stay compatible. Assertions, network applications
and formal review/QA remain deferred; scoped formatting and the exact four
compile-only checks follow both paired writers' terminal stop.

The four scoped compile-only configurations have now completed with exit 0;
their scope and the six unexecuted fixture groups are recorded in
`doc/changes/CHANGE__competition-progress-port.md`. This evidence does not
establish runtime, platform, performance or independent review/QA acceptance.

## Known ceiling

Endpoint acquisition, native start/cleanup owners and some retained prefix
allocations remain in outer owners. Group cadence now uses an injected clock
through [the cadence contract](REQ__competition-progress-cadence.md), with
native time acquisition confined to its adapter. This increment separates
progress policy and does not claim complete IO-layer separation, allocation-free
networking, real QUIC/WebTransport acceptance or measured latency improvements.
