# Room clock and start control framing

BKMR version 2 retains the admission message payloads, 11-byte header and
65808-byte frame bound while adding exact ClockPing, ClockPong and existing
StartMessage control frames. Version 1 and bilateral BKMP are rejected. Probe
sequences retain positive full-width u64 values; nonnegative timestamps retain
the full i64 range. Pong validates only remote receive/reply ordering, preserving
independent clocks and signed offsets. Each header validates exact fixed control
payload sizes before admitting a body. Encoding validates before allocation;
incremental malformed-state retention and coalesced suffix ownership remain.

Production and independent codec fixtures are saved. Three new codec groups
bring that fixture to nine while preserving all six prior admission/decoder
groups. Control literals, full-width values, independent clock origins, malformed
sizes/values, retained failures, fragments/coalesced suffixes and actual
StartAgreement messages are covered in source; no assertions were executed. The
existing client literal Join header has been aligned to v2, and a new deferred
client group pins refusal of clock/start traffic after actual prepared admission
without changing its lease, roster or complete-write state. The existing server
forged-message group now covers all new control variants. Neither admission-only
owner grants start permission from recognizing a valid frame.

No probe correlation, complete clock/start write receipts or timing activation
has been composed into the server/client/Worker yet. The common coordinator and
wire representation prepare those required next steps; multi-host progress and
real final ACKs remain unfinished. Tests, runtime, device/TLS/browser operation,
generated bindings and formal review/security/QA remain user-deferred. Scoped
Rust formatting followed actual producer and independent author STOPPED states.
Four locked compile-only checks exited 0: workspace all targets with webtransport
(68043), headless app all targets with webtransport (26792), WASM browser (65871),
and WASM browser-audio (69953). Existing WASM audio cadence dead-code warnings
remain. Linux/WASM compilation does not establish active Windows/macOS paths,
physical devices or browser/network execution.
No interoperability, physical synchronization, distributed atomic delivery or
ranked-score authority is claimed. Ordered review/security and browser/CLI/desktop
QA are mandatory before eventual close; the full Goal remains active and the
Harness task stays open/PENDING.
