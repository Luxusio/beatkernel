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

Review-driven fixes prevent late success after timeout, reject altered actual
navigation HTML, and require correlated Results RPC/state acknowledgements,
draw and positive geometry instead of packet arrival alone. Profile ownership
is canonicalized through real existing ancestors before launch and cleanup;
nested or symlink-aliased profiles are refused. Production source is unchanged.
Final DEEP code and trust/resource review PASS at `9ce2670`; independent CLI
QA reports68/68 tests (21 new portable gate cases plus47 existing cases), both
syntax checks PASS and actual missing-configuration entrypoint exit1 before
browser launch. Independent browser QA returns BLOCKED_ENV after inspecting
raw preflight evidence; no new browser or gameplay run is claimed. Review
results are non-attesting when hook-owned receipts are absent and do not
authorize task closure.

## Known ceiling

Strict browser acceptance remains blocked by the available local TLS/QUIC trust
environment. Production gameplay, both natural completions and native/browser
interoperability remain unverified here. WBS12.04 stays V and overall WBS89/193
is unchanged. Source review, syntax/Node checks, actual browser acceptance and
hook-owned task closure are separate outcomes.
