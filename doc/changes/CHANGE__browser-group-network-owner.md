# Browser group network owner

The existing Worker-side BrowserMultiplayerOwner gains explicit group mode,
bounded immutable member-word submissions and a combined group/lifecycle event
drain. Both modes use the same transport, elapsed clock, write admission,
full-write receipt, final ACK waiter, cancellation and Session disposal. Input
snapshots are bounded before generated WASM glue; Rust remains authoritative
for rosters, cumulative progress and sequence. No Window gameplay rendering or
protocol fork is introduced.

Six deferred test groups were added to the actual owner-module fixture, bringing
its total to thirteen while preserving the seven existing groups. They cover
mode capabilities, immutable full-width snapshots and bounds, combined event
capacity, full-write/ACK separation, callback disposal and late completion.
Scoped whitespace checks completed. No JS parser, assertions or runtime was
executed, and no Cargo checks were repeated for this JS-only change.

Known ceiling: Actual Page/Worker group launch, canonical identity selection,
remote member HUD targeting, native callers and multi-host rooms remain pending.
Owner fixtures are authored for later execution; source changes do not prove
generated bindings, sockets, browser/device execution or performance. Formal
review, QA and close remain deferred under the standing user instruction.
