# Strict actual two-host WebTransport player verification

The new actual-play acceptance runner requires current production assets, two
independently focused browser hosts, real Play gestures and keyboard input,
matching reported peer/local scores and both natural final write/ACK/drain
outcomes. It complements the existing direct room protocol test; constructed
protocol score words and one completed peer do not prove full gameplay.
The [runtime contract](../runtime/REQ__browser-room-play.md) and
[verification guide](../verification/GUIDE__browser-room-acceptance.md) define
the intended behavior and exact setup boundary.

Current local preparation accepts ordinary HTTPS with disposable NSS CA trust
and rejects an unrelated CA. Actual unchanged WebTransport stream acquisition
fails the opening certificate handshake. Original NetLog, errors and terminal
cleanup are preserved; no certificate bypass is used. Related Node owner/
transport tests pass47/0. The actual-play runner is prepared for an eligible
trusted endpoint, but full two-host execution is not established on this setup.

## Known ceiling

Strict browser acceptance remains blocked by the available local TLS/QUIC trust
environment. Production gameplay, both natural completions and native/browser
interoperability remain unverified here. WBS12.04 stays V and overall WBS89/193
is unchanged. Source review, syntax/Node checks, actual browser acceptance and
hook-owned task closure are separate outcomes.
