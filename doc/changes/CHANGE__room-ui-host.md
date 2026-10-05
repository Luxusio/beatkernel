# Room controller UI host injection

Native room controller UI effects use an owned generic RoomUiHost, covering
attachment/cancellation, request/reply flow, control closure, retained room
publication/retry and results publication. new_with_host permits deterministic
injection alongside the existing network port. The old constructor/default host
remain compatible through an outer bridge to actual player UI functions.

Controller production logic no longer selects a concrete player implementation.
Existing bounded UI/network request correlation, retry without duplicate network
commands and presentation failure isolation stay in the actual controller. The
host boundary uses existing cold IO/String errors; no new per-note trait object,
crate or queue is introduced.

Native network/owner internals, result construction and cleanup diagnostics remain
further separation work. This boundary does not prove whole pure-controller IO
independence or measured zero overhead.

Independent fixtures use the actual injected controller with fake UI and network
ports, without attaching the native player. Assertions, native UI/device/thread/
network/browser execution, benchmarks and formal review/QA remain deferred.
Scoped formatting and four compile-only checks wait for both terminal writer
stops. The full player Goal/Harness task remain open without acceptance PASS.

Both paired writers returned terminal Writes STOPPED before scoped Rustfmt of
five changed Rust paths. Four compile-only checks ran sequentially, exiting zero:

- Workspace all targets, runtime webtransport (session 7585).
- Runtime all targets, no defaults, webtransport (session 11930).
- WASM library, no defaults, browser (session 19318).
- WASM library, no defaults, browser-audio (session 18725).

Six independent groups instantiate the actual injected controller and compile
in host all-targets checks. No assertion ran. They cover detached effect gates,
retained Waiting publication/retry, initial/poll cancellation, original UI vs
network command IDs under reply contention, local actions/projected errors, and
cancelled join/results with retained cleanup failure and missing receipts.
WASM checks compile the portable UI contract, not the native controller fixtures.
Pending startup/natural-finish waits and native UI/network/join behavior remain
unverified. Existing dead-code warnings remain; compile results are not PASS.
