# Portable room network contract

Room network options, commands, errors, replies, rosters, terminal outcomes and
snapshots belong to a portable value module. It imports no thread, socket,
renderer, native endpoint or concrete player. Original PlayerId/ParticipantId,
sequence/revision, signed deadlines, schedules and historical receipts retain
their existing exact types, ordering and ownership.

RoomNetworkOptions preserves current bounded validation: queue capacity 1..1024,
setup/drain/finish durations 1 ms..120 s, valid start policy and nonnegative
preroll. Validation is callable without acquisition. Native worker constructors
and actors reuse this same validator before effects rather than duplicate limits.

RoomNetworkPort defines command admission, poll, original room-clock observation,
stop request and joined outcome using only those values and current IO errors.
The native thread owner implements that contract outside policy. Controller
logic consumes the portable contract/values; existing NativeRoomPort and all
native model import names remain compatibility aliases to the same definitions.

Command admission returns its original u64 ID, not a UI ID or peer receipt. Poll
snapshots and outcomes are evidence values that existing consumers validate;
constructing or cloning them cannot establish real Commit, ACK or completion.
Cleanup errors and historical receipts remain independent fields. Existing Arc
ownership of retained rosters/prefixes remains unchanged; no policy serialization,
new queue or per-note allocation is introduced by this extraction.

The [portable controller](REQ__portable-room-controller.md) receives all hosts
explicitly. Native defaults and physical start/wait/lifecycle adapters remain
outside it; worker stream/time/thread ownership and browser adapter integration
remain further work. Extracting the contract is a real dependency boundary,
not proof of complete IO separation or equivalent adapter behavior.

Independent deferred fixtures cover option bounds, exact identity/time/value
ownership and injected port contracts without native owners. Host/WASM compilation
is source evidence only; assertions and real native/network/platform/benchmark
execution plus formal review/QA remain deferred.
