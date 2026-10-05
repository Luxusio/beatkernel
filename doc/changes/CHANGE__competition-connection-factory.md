# Validated competition connection factory

Competition acquisition uses a business-owned generic factory. Requests borrow
the role and own existing identity/roster/options. Preflight checks actual common
limits and role-specific metadata before native acquisition. Original player
order/identities, vector allocations and opaque factory errors cross unchanged.

NetworkRole and MultiplayerOptions move to policy value modules with compatible
re-exports. Common options and bilateral setup-size rules remain shared with
the actual multiplayer constructors/protocol encoder. Room requests retain the
room identity bound rather than inheriting a bilateral envelope constraint.

The native competition constructor captures real graphical room availability at
the outer boundary and supplies an actual factory for existing bilateral/room
owners. No runtime, socket, TLS bytes or player implementation enters policy.
Concrete owner internals and native room/ACK waits remain unfinished boundary
work; full IO independence and whole-player acceptance are not established.

Implementation and independent fake-factory fixtures are paired. Test execution,
native IO/TLS/network/browser/device/graphics/benchmarks and formal review/QA
remain deferred. Scoped formatting and allowed compile checks wait for both
writers' terminal stops. The full Goal and Harness task remain open.

Both paired writers returned terminal Writes STOPPED before scoped formatting
of the nine changed Rust paths. These compile-only checks ran sequentially,
each exiting zero:

- Workspace all targets, runtime webtransport (session 3074).
- Runtime all targets, no defaults, webtransport (session 73739).
- WASM library, no defaults, browser (session 32029).
- WASM library, no defaults, browser-audio (session 18790).

Eight cfg-dependent fixture groups are authored: six common groups, a supported
WebTransport/room group and an unsupported-build group. Host all-targets checks
compiled the applicable seven groups; the unsupported-build assertions were not
test-target compiled by the permitted WASM library checks. No assertions ran.
Existing dead-code warnings remain. These source results do not establish
actual resource cleanup, native acquisition, peer agreement or whole-player
completion.
