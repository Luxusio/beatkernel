# Common room progress relay and server ownership

Connect portable participant progress/final receipt ownership to the actual
WebTransport PreparedRoom actor. Preserve original stream leases, committed-start
write barriers, bounded shared-prefix fanout and one real in-flight receipt per
recipient. Genuine per-recipient final ACKs produce one aggregate source ACK;
queueing or partial writes cannot complete the room.

Implementation and independent deferred fixtures are saved: six new common
relay groups and two new server groups, preserving all eight previous server
groups. Both writers stopped before scoped Rust formatting and whitespace checks.
Four compile-only Cargo checks succeeded: workspace/headless WebTransport and
WASM browser/browser-audio. Existing three WASM cadence dead-code warnings remain.
No tests, live TLS/network/browser/audio/device runtime, generated bindings or
review/QA acceptance are claimed. Client gameplay publication/HUD/final drain
and native app room activation remain unfinished. The full Goal remains active.
