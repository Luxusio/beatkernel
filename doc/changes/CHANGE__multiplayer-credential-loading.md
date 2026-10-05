# QUIC credential loading boundary

QUIC credential metadata and role validation move to portable policy, with the
existing import path preserved as a re-export. A generic reader prepares owned
certificate/key or CA bytes after whole-role validation. It preserves original
borrowed native paths, server-name borrowing and opaque read-error identity.
Empty or oversized adapter responses refuse further preparation.

The actual native QUIC endpoint uses a filesystem reader adapter. Existing
address preflight, regular-file checks, read bounds, TLS decoding/trust policy and
socket binding remain native. WebTransport CA acquisition and full endpoint
factory injection remain outstanding; this does not claim complete native IO
separation.

Implementation and independent fixture authoring are paired. Source formatting
and the four allowed compile-only configurations wait for both terminal writer
stops. Behavioral assertions, platform/TLS/network execution, performance
measurements and ordered formal review/QA are deferred. The full player goal and
Harness task remain open, without an acceptance PASS.

Both paired writers returned terminal Writes STOPPED before scoped Rustfmt of
the four changed Rust paths. The following checks ran sequentially and each
exited zero:

- Workspace all targets with runtime webtransport (session 68087).
- Runtime all targets, no default features, webtransport (session 27009).
- WASM library, no default features, browser (session 49932).
- WASM library, no default features, browser-audio (session 85141).

Six fixture groups (five portable and one Unix-only) were authored. Host
all-targets checks compiled the applicable fixtures; none of their assertions
were executed. Existing dead-code warnings remained. These compile results do
not establish native file/TLS/network behavior or browser acceptance.
