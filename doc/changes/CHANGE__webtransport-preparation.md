# WebTransport metadata and injected CA acquisition

Destination, Origin and CA-path validation move into a policy module without
imports of concrete network/TLS implementations. Client options and validator
import paths remain compatible; relay Origin admission delegates to the same
validator. Existing URL parser, canonical rules and feature/build availability
are retained.

The actual native endpoint prepares trust bytes through the existing generic
credential reader contract. Complete metadata validation precedes the single CA
read. Policy retains original option/key references, moves the owned CA buffer
and returns original associated errors. TLS decoding/configuration, binding
family selection and transport/socket/runtime ownership remain native.

Implementation and independent fixture authoring are paired. Test assertions,
filesystem/TLS/network/browser execution, formal review/security/QA and close
are deferred. Scoped formatting and the four compile-only configurations wait
for both writers' terminal stops. The full player goal remains unfinished.

Both writers returned terminal Writes STOPPED before scoped Rustfmt of the seven
owned Rust paths. Sequential compile-only results:

- Workspace all targets, runtime webtransport: exit zero (session 39882).
- Runtime all targets, no defaults, webtransport: exit zero (session 74561).
- WASM library, no defaults, browser: exit zero (90601), with a new unused
  StartRole import warning. The owning worker gated only that import to native
  WebTransport builds and stopped; the affected file was formatted, and the
  browser check repeated with exit zero and the new warning absent (88931).
- WASM library, no defaults, browser-audio: exit zero (session 40036).

Eight cfg-dependent fixture groups are authored: six supported-build groups,
one supported Unix group and one unsupported-build group. Host all-targets
checks compiled their applicable seven groups; unsupported-build assertions
remain authored without test-target compilation in the permitted WASM library
checks. No assertion ran. Existing dead-code warnings remain. These results
do not establish filesystem, TLS, sockets, browser behavior or performance.
