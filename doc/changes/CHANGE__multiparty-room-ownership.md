# Multi-host room ownership foundation

A distinct common GroupRoomRegistry is implemented for 2..64 network
hosts, each with its own ordered 1..64 player roster and matching canonical
setup identity. Player identities remain scoped by the issued nonreused u64
participant lease, allowing identical local PlayerIds on different hosts.
The bilateral RoomRegistry and HTTP/3 relay keep their existing wire semantics.
Both registry modes share the exact bounded ASCII key policy.

The room creator explicitly seals membership. Every frozen participant reports
preparation once; the original waiting deadline remains until all are prepared.
This is not a clock/start commitment, transport write or final ACK. Expiry,
release and irreversible stop return exact closure tickets, preserve ownership
on rejected operations and prevent stale leases from closing replacement rooms.
The caller still owns and closes actual streams.

Production and six independent deferred fixture groups are authored. Fixtures
cover bounded admission, scoped rosters, authority/readiness, stale leases,
expiry and checked clock/ID limits, and irreversible stop. They have not run.
Scoped Rust formatting and four compile-only checks completed successfully:
workspace all targets (64873), headless app all targets (5989), WASM browser
(92204), and WASM browser-audio (49420), all locked, exit 0. Existing WASM
audio cadence dead-code warnings remain. No tests or runtime checks ran.
These checks do not establish active Windows/macOS code or device acceptance.
Actual room wire
negotiation, per-host software clocks and output starts, bounded fanout and
backpressure, participant-scoped progress/final ACKs and native/browser callers
remain required next work. Source/compile evidence will not prove playable
multi-host networking or physical/performance acceptance. The full player Goal
and Harness task remain active/open with formal review/security and required
browser/CLI/desktop QA deferred and mandatory before eventual close.
