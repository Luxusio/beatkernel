# WebTransport group admission

The existing multiplayer server gains an explicit `--group-hosts N` route for
BKMR room admission, sharing the existing TLS and request policy. Each accepted
stream first supplies canonical identity and its ordered local roster. The
server assigns a participant lease and sends admission before the full room
snapshot; later seal/readiness/leave requests are bound to that stream lease.
Default mode retains the bilateral BKMP relay.

The common room registry owns membership and expiry; actual server resources
own connections, streams and bounded reader/writer tasks. Snapshot queue refusal,
malformed input, peer termination and expiry close the exact affected room.
Stale task completion cannot close a replacement room. Idle command streams
wait without an artificial I/O deadline; a begun frame and each complete write
use the configured deadline. Shutdown closes resources and joins tasks.

Production source and six independently authored helper fixture groups are
saved, together with group-mode assertions in the existing configuration group.
The helpers exercise real duplex framing/deadlines, ordered/cancelled writes,
2/3/4/64-host snapshots, stream-bound requests and atomic bounded fanout refusal.
They do not execute live TLS, resource teardown or semaphore acceptance.
Scoped Rust formatting and four locked compile-only checks completed with exit
0: workspace all targets with beatkernel-bms-runtime/webtransport (54920),
headless app all targets with webtransport (3790), WASM browser (2483), and WASM
browser-audio (91344). Native checks explicitly included the optional server
code and fixtures. Existing WASM audio cadence dead-code warnings remain.
Linux/WASM compilation does not establish active Windows/macOS or devices; no
fixtures or server/browser/native endpoints ran.
This route is
admission/readiness only: multi-host clock/start agreement, gameplay progress,
final ACKs and native/browser gameplay integration remain required. Tests,
endpoint/browser/native execution, formal reviews/security and required
browser/CLI/desktop QA remain user-deferred and mandatory before eventual close.
The full player Goal and Harness task stay active/open.
