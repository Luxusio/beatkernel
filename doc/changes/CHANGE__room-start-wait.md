# Portable room startup waiting

The actual native room startup trait delegates waiting to generic initial/poll
observations, an injected service callback and a narrow wait port. Native UI
cancellation/polling remain adapter effects; sleeping lives in the private room
wait bridge without acquiring a new startup clock.

Initial and polled cancellation, service/poll errors, Leave and terminal history
retain their precedence over Commit. Pending loops request the existing 1 ms
wait and run service again before polling. Original opaque service/poll/wait
errors cross the policy boundary unchanged. No alternative setup timer or
physical-start proof is introduced.

Existing outer cancellation/stop and network-error reporting remain unchanged.
Native UI/owner internals and network worker waits remain further separation
work. This is an actual startup-path integration, not complete platform/player
acceptance.

Implementation and independent scripted fixtures are paired. Assertions and
native UI/thread/network/browser/platform/benchmark execution plus formal
review/QA remain deferred. Scoped formatting and four compile-only checks wait
for both terminal writer stops. Full Goal/task remain open without PASS.

Both paired writers returned terminal Writes STOPPED before scoped Rustfmt of
five changed Rust paths. Four compile-only checks ran sequentially, exiting zero:

- Workspace all targets, runtime webtransport (session 77675).
- Runtime all targets, no defaults, webtransport (session 97122).
- WASM library, no defaults, browser (session 53365).
- WASM library, no defaults, browser-audio (session 78025).

Six independent fixture groups were authored and compiled by host all-targets
checks; no assertion ran. They cover four initial gates, 31 nonpending poll
combinations, service cancellation/error, repeated pending state, wait failure
identity and a stack-borrowed unsized callback. Original service/poll/wait traces
are scripted without clocks or threads. Native UI/owner behavior, network
timeouts, real Commit provenance and activation remain unverified. Existing
dead-code warnings remain; the compile results are not acceptance PASS evidence.
