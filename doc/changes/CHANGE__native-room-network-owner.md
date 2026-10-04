# Shared native room transport ownership

`NativeRoomNetwork` now drives actual `RoomPlayIo` on one dedicated network
thread. Its WebTransport constructor prepares credentials and calls the actual
`WebTransportEndpoint::connect_room_play` on that thread. Native audio/input/UI
adapters retain their own resources; this module adds no operating-system room
protocol, dependency or crate. The module is excluded from WASM.

One queue bound of 1..1,024 covers queued, processing and unread replies together.
Commands use positive u64 correlation identities, and full/busy refusal consumes
neither an identity nor a credit. Seal, Ready, Leave, Publish and explicit Drain
use the actual common protocol owner. Local phase refusals return correlated
errors without stopping a healthy connection. Polling releases reply credits;
terminal settlement refuses commands not yet processed.

One Instant origin precedes asynchronous connection and supplies checked i64
elapsed observations. Setup timeout covers connection through genuine start
commitment; the explicit drain timeout covers waiting for local completion
through genuine DrainComplete. Neither is renewed. Checks before and after I/O
refuse a deadline crossed by a bounded stream operation, even when a late real
receipt has arrived. Historical local-final-written, local-final-ACK,
progress-complete and drain-complete proofs stay distinct and survive later
failure. They do not replace the actual live protocol state or successful
terminal outcome.

Room metadata and qualified peer prefixes are retained behind Arc. Metadata
copies occur only on accepted changes; peer payloads copy only on sequence
changes. Full roster order and host-scoped player identity survive independent
peer arrival order. Polling copies Arc references rather than all player data.
Native host/output projection remains the caller's responsibility; a committed
software schedule is not physical synchronization evidence.

Stop and Drop cancel and join acquisition, I/O and stream finish before releasing
thread ownership. Operational and cleanup errors remain separate in a retained
terminal outcome. The production-used Actor seam permits deterministic scripted
stream fixtures over actual registry, clock, coordinator and progress relay.
Independent fixture source adds six groups, including genuine 2/3/4-host flows,
bounded requests, cancellation/join, fixed and post-I/O deadlines, retained
historical receipts and cleanup failures. Fixtures have not been executed.

Both writers actually stopped before scoped rustfmt and four compile-only Cargo
checks. Workspace/all-targets with WebTransport, runtime no-default/all-targets
with WebTransport, WASM browser and WASM browser-audio all exited 0. Whitespace
inspection also completed. The WASM checks retained three existing cadence
dead-code warnings. No tests, JavaScript parsers, applications, generated bindings,
network/device runs, formal reviews or QA were executed.

Known ceiling: generic connectors and streams must honor supplied cancellation,
absolute deadline and bounded-I/O requirements; arbitrary blocking custom code
cannot be forcibly joined within a promised duration. Explicit Stop returns
cleanup errors; Drop can only report them diagnostically. Native app lobby,
cohort/start activation, portable score HUD and natural finalization callers
remain subsequent integration work. Raw QUIC multi-host integration, live
TLS/browser/device behavior and measured performance remain unverified. The
full Goal and Harness task stay open, with no runtime PASS or close claimed.
