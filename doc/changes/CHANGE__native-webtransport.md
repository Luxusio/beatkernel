# Native gameplay through the WebTransport relay

AC-198 continues the open player task after the actual HTTP/3 relay source
milestone. The same application crate adds an optional native WebTransport
client and selects it through the existing common networking worker. No
operating-system-specific multiplayer branch, application crate or dependency
is introduced. The kernel remains independent of transport libraries.

`--mp-webtransport HTTPS_ROOM_URL --mp-role host|join --mp-origin ORIGIN
--mp-ca PATH` selects this transport. Both participants connect to the relay as
clients; `host` proposes the software start and `join` accepts it. Raw QUIC
`--mp-host`/`--mp-join` remains available. Modes are mutually exclusive, and
WebTransport rejects host certificate/key and a separate TLS server-name
override because the HTTPS URL supplies the server identity. The actual shared
Session retains chart compatibility, readiness, clock probes, committed start,
progress and final peer ACK behavior.

Endpoint and stream enums choose the concrete transport around the one existing
Multiplayer worker. Socket/DNS/HTTP3 connection and stream I/O stay on that
worker. The client borrows one pinned setup future across bounded five-millisecond
cancellation/deadline ticks. Actual stream reads/writes preserve native
backpressure semantics; whole setup and finish/drain remain finite. Shutdown
closes actual connection/endpoint owners, and Drop does not wait. Pinned
wtransport uses Tokio's resolver; a blocked OS DNS operation cannot necessarily
be cancelled. The runtime owner uses non-waiting shutdown after closing network
resources, so that pending blocking resolution cannot hold the worker join.
Late resolution results are fenced; this does not prove immediate OS resolver
resource release.

A canonical bounded HTTPS `/rooms/KEY` URL and exact configured serialized
Origin are required. Trust comes from an explicit bounded regular-file
certificate bundle, parsed by the existing genuine rustls root configuration.
TLS uses TLS1.3/ring/h3 with early data disabled. There is no verifier bypass,
implicit trust fallback, endpoint upload of chart assets or ranked authority.
Required DATAGRAM capability is advertised with bounded buffers while gameplay
uses only the reliable stream.

Common native settings retain URL, role and Origin drafts on all three hosts.
Explicit transport overrides replace the prior role/credential/Origin family
together, preserving unrelated audio, gameplay and timing options. Missing CA
remains a draft until final admission. A build without `webtransport` can retain
a draft and explicitly refuses final connection admission before reading
credentials or constructing runtime/socket owners. Desktop practice-loop
controls recognize this network mode and preserve the existing bilateral
restriction.

Independent fixture authorship targets actual URL/origin/TLS configuration,
common argument extraction/conflicts, settings overlays and disabled-feature
admission. No handshake simulation proves interoperability. Assertions, sockets,
TLS/browser/native execution, audio, formal review and QA remain deferred under
the standing source-first instruction. The Goal stays active and the task stays
open/PENDING.

After both writers stopped, WebTransport headless/all-targets, desktop with
WebTransport/all-targets and both WASM browser/library paths returned exit0.
Workspace/headless checks initially found a missing test-only TLS helper
reexport; after its scoped correction both reruns returned exit0. Thus all six
planned compile paths have genuine successful terminal results, all `--locked`.
Five independent client groups compile without the feature and seven with it;
assertions remain unexecuted. WASM retained the three existing cadence dead-code
warnings. Scoped formatting and staged whitespace produced no diagnostics.
No runtime, interoperability, formal review, QA or acceptance claim follows.
