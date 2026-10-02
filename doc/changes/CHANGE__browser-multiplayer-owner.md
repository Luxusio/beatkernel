# Browser gameplay compatibility and multiplayer ownership

Canonical replay header construction is shared by capture and competition
identity queries. Portable gameplay derives identity from its actual pristine
judge, profile and chart seed, using the same runtime version and normalized
competition encoding as native play. BrowserGame exposes this bounded identity
without enabling recording or changing an existing capture. Setup queries
reject after gameplay starts or is fenced.

The browser multiplayer owner composes the existing Rust Session bindings and
WebTransport byte channel. It drives bounded read prefixes, actual control
write admissions, exact frame receipts, event forwarding and an explicit local
elapsed clock. One pending application submission is permitted. A submission
resolves on complete local write; the final peer application ACK remains a
separate observation. The owner frees consumed WASM write objects before
waiting on transport and never calls freed session state after cancellation.
Completion and ACK bookkeeping precede consumer callbacks, preserving observed
receipts even when a callback immediately closes the owner.

A finite absolute setup deadline covers connection and shared preparation.
Duplex loops use an interruptible tick when waiting. Cancellation, timeouts,
transport or session errors fence late results and reject pending callers;
cleanup releases owned session state once. Readiness is explicit after actual
preparation. Software start events do not implicitly start audio.

Configuration preflight failures leave the supplied WASM session caller-owned.
After preflight succeeds, the owner consumes it before reading the clock;
connection, clock or cancellation failure thereafter closes and frees it once.
The injected clock returns absolute nonnegative BigInt nanoseconds; only the
elapsed difference is limited to signed 64-bit extent. The caller still checks
target mapping against its gameplay clock's admitted range.

After both writers stopped, workspace/all-targets, headless/all-targets,
browser-WASM and AudioWorklet-WASM `cargo check` each exited zero. Five Rust
identity groups were compiled and seven JavaScript owner groups were authored;
none of their assertions or JavaScript syntax checks ran. Existing WASM platform
dead-code warnings remain. The detached-chunk source fix uses its already
authored negative fixture, also unexecuted.

Known ceiling: the existing Window/Worker Play controls still need integration,
including caller clock mapping and actual audio startup at the committed target.
A compatible HTTP/3 service and native interoperability remain unfinished.
Independent assertions, browser/network/hardware execution and formal review/QA
remain deferred; these source changes do not complete the full player Goal.
