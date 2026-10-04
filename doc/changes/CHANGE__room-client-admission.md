# Common room admission client

One common BKMR client retains the exact requested identity/local roster,
accepts its assigned participant and complete ordered room snapshots, and emits
role-valid seal/readiness/leave requests. Full-write receipts, immutable frozen
membership, append-only collecting membership and monotonic preparation prevent
partial writes or changed snapshots from confirming admission/readiness.

A bounded Read/Write driver retains frame offsets and fragmented input across
WouldBlock. The existing trusted native WebTransport endpoint gains a connector
for this driver. The browser byte channel can explicitly select a prefix limit
up to the 65808-byte room-frame bound, retaining its 65547-byte default, fixed
chunk bound and existing cancellation ownership. This does not move browser
networking onto the Window main thread.

Production and six independently authored Rust fixture groups are saved, along
with one additional browser byte-channel fixture (nine groups total there).
The Rust fixtures cover full-write barriers, Registry-produced host snapshots,
atomic refusal, request roles, fragmented I/O, WouldBlock/Interrupted and fatal
count/EOF/protocol fences. Browser source fixtures cover explicit/default limits,
owned maximum-size writes and coalesced suffixes. No fixtures have been run;
JavaScript has not been parsed or executed.

Scoped Rust formatting and four locked compile-only checks completed with exit
0: workspace all targets with webtransport (97398), headless app all targets
with webtransport (85223), WASM browser (58307), and WASM browser-audio (33534).
Existing WASM audio cadence dead-code warnings remain. Linux/WASM compilation
does not establish active Windows/macOS, TLS or physical/device acceptance.
Native
and browser gameplay callers, generated browser bindings/Worker room ownership,
multi-host clocks/shared start/progress/final ACKs and actual interoperability
remain unfinished. Test and runtime execution, formal reviews/security and
required browser/CLI/desktop QA remain user-deferred and mandatory before close.
The full player Goal and Harness task stay active/open.
