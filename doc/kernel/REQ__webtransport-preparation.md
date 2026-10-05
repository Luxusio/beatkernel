# WebTransport trust preparation

WebTransport destination, Origin and CA-path metadata validation belongs to
policy, separate from network/TLS implementations. Existing client options and
validation import paths remain compatible. URL and Origin validators retain
their current canonical rules using the existing URL parser; no handwritten
replacement parser or new dependency is introduced.

A generic credential reader prepares CA bytes only after validating the complete
options. Invalid URL, Origin or CA-path metadata performs no read. Both bilateral
start roles are clients and read only CA trust material. Reader errors retain
their original opaque identity without extra trait bounds; empty or over-1-MiB
responses refuse preparation. Original options are borrowed and owned CA bytes
move through policy without copying or path/display formatting.

Native endpoint preparation delegates acquisition through this policy with the
existing bounded filesystem reader adapter. TLS trust-store/configuration and
binding-family selection remain native. The relay server uses the same Origin
validator, preserving admission semantics. Endpoint connection, native clocks,
runtime/socket ownership, room waits and DNS remain further boundary work.

Feature/build availability remains unchanged: without native webtransport
support, option validation/preparation returns Unsupported before any reader
effect. This does not add native endpoint support to WASM; browser transport
remains its own adapter. Destination rules can be tested without sockets or TLS
where the existing parser feature is available.

Independent fake-reader fixtures cover complete preflight, canonical URL/Origin
rules, original keys/options, both start roles, moved byte buffers, byte bounds,
opaque error identity and unsupported builds. Assertions, actual filesystem,
TLS/network/browser execution and formal review/QA remain deferred. Compile
checks are source evidence only.

Eight cfg-dependent groups are authored. The four permitted configurations
exited zero, with an affected WASM check repeated after an unused import fix.
See [the evidence scope](../changes/CHANGE__webtransport-preparation.md).
