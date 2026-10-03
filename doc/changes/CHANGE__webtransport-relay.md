# Optional HTTP/3 WebTransport relay

AC-197 continues the open BMS player task. The existing application crate adds
an optional native `webtransport` feature and `serve-multiplayer` mode. The
std-only kernel and portable room/session components remain independent of
Tokio, Quinn, TLS and HTTP/3. No additional application or crate is introduced.

The adapter uses actual wtransport 0.7.2 HTTP/3 sessions over the existing
Quinn/Tokio dependencies, with default features disabled and explicit ring/quinn
support. url 2.5.8 parses exact canonical origins. Upstream APIs were checked
against the [pinned server example](https://github.com/BiagioFesta/wtransport/blob/d022a6ad5f6260cb526e06d8ca42d09516ce6440/wtransport/examples/server.rs)
and [TLS implementation](https://github.com/BiagioFesta/wtransport/blob/d022a6ad5f6260cb526e06d8ca42d09516ce6440/wtransport/src/tls.rs).
New package license texts and pinned-revision provenance are retained in the
application's third-party notices. Authored sources remain MIT; optional ASIO
distribution policy is unchanged.

Each session supplies exactly `/rooms/KEY`, where KEY uses bounded ASCII
letters, digits, hyphen and underscore. Queries, fragments and encoded traversal
are rejected. The request Origin must match an explicitly configured serialized
origin. HTTPS is accepted, with HTTP restricted to loopback page origins, in
line with [secure-context loopback treatment](https://www.w3.org/TR/secure-contexts/#is-origin-trustworthy).
The WebTransport endpoint always uses HTTPS. Missing Origin is rejected unless
the caller explicitly selects `--allow-missing-origin`; this is not peer
authentication. No ranked result or trusted score authority is introduced.

Certificate and private-key files have a 1 MiB per-file ceiling. Genuine rustls
PEM parsing validates the certificate chain and supported private key before
server construction. There is no generated self-signed certificate, trust bypass
or private-key printing. Configuration bounds cover rooms, keys, total sessions,
concurrent setup, setup duration, waiting duration and whole-frame I/O duration.
WebTransport advertises the required QUIC DATAGRAM capability with bounded
16 KiB receive/send buffers; application traffic uses only the reliable stream.
Datagrams are not forwarded. This follows the
[HTTP/3 WebTransport establishment requirements](https://datatracker.ietf.org/doc/html/draft-ietf-webtrans-http3-12#section-3.1).

The server admits a participant to RoomRegistry only after its actual
bidirectional stream exists. Transport owners use the returned monotonic leases
to close exact sessions, including expired waiters and both members of a pair.
Stale completions cannot release newer same-key rooms. One common BKMP v6
FrameDecoder and encoder validate and forward frames in each direction under
backpressure. The relay does not duplicate Session compatibility, judging,
readiness, software clock agreement or peer-final-ACK semantics.

Normal EOF half-closes one direction and gives the opposite direction at most
two seconds for the final application ACK. Malformed/truncated frames, I/O
timeouts, transport errors and cancellation close both participants. Ctrl+C
stops acceptance, closes transport owners and aborts/joins bounded setup and
relay tasks. Late setup results are disposed rather than admitted after shutdown.

Independent fixtures target actual configuration/admission/PEM and Tokio duplex
relay code, including backpressure, reverse ACK after EOF, malformed/truncated
frames, cancellation and finite deadlines. Assertions, sockets, TLS/browser
interoperability, audio execution, formal review and QA remain deferred by the
standing source-first instruction. Native gameplay still uses raw QUIC; native
WebTransport gameplay selection is separate follow-on work. The persistent Goal
remains active and the task remains open/PENDING.

After both writers stopped, five real compile-only checks returned exit0:
workspace/all-targets, application headless/all-targets, native webtransport/
all-targets, WASM browser/lib and WASM browser-audio/lib, all with `--locked`.
The optional native path compiled the eight fixture groups without running
assertions. Both WASM paths retained the three existing platform cadence
dead-code warnings. Scoped formatting and whitespace checks produced no
diagnostics. These results establish source compatibility, not server or
interoperability acceptance.
