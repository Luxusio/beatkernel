# Portable final ACK wait loop

Scalar and group final-delivery waiting delegates to portable policy with
generic observation/admission and time/park ports. Notice draining, native
cancellation and message ownership stay in the multiplayer adapter; Instant
acquisition and thread parking move to a private bridge.

The fixed deadline is an exact unsigned integer in the supplied wait domain.
Retries never extend it. Policy preserves cancellation/disconnect/ACK ordering,
retryable queue pressure and original port/control errors. Monotonic pre/post
observations bound parking after admission without guessing peer delivery.
Successful admission remains distinct from actual final acknowledgement.

Both scalar and group production methods use this path. The original native
batch-last disconnect precedence remains; room final/drain waits and full native
owner construction are unfinished. Cleanup ACK waiting is separate from song
timing, physical output completion and result evidence.

Independent scripted fixtures are authored without native clocks/channels,
thread parking or transport. Assertions, native IO/platform/browser/network,
benchmarks and formal review/QA remain deferred. Scoped formatting and the four
compile-only configurations wait for both writer stops. Full Goal/task stay open.

Both writers returned terminal Writes STOPPED before scoped Rustfmt of five
changed Rust paths. Four compile-only checks ran sequentially and exited zero:

- Workspace all targets, runtime webtransport (session 70653).
- Runtime all targets, no defaults, webtransport (session 34753).
- WASM library, no defaults, browser (session 30847).
- WASM library, no defaults, browser-audio (session 20301).

Eight independent fixture groups were authored and host all-targets checks
compiled them. No assertion was executed. They use scripted clocks/ports for
terminal precedence, fixed deadlines, queue retries, opaque error identity,
large integer times, regression and post-admission overshoot. Native full-batch
notice behavior, message copying, real acknowledgement, clock acquisition and
thread parking remain unverified by these pure fixtures. Existing dead-code
warnings remain. Compile results are not acceptance PASS evidence.
