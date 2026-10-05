# Native pump clock and wait boundary

Common solo/local native loops now use injectable control time and waiting while
retaining existing native entry points. The system adapter owns `Instant` and
sleep; the generic loop retains original diagnostic cutoff and output evidence
semantics. Clock regression and control read/wait failures propagate explicitly.
Record the
user's performance, deterministic-testability and explicit UI/Business/Native IO
separation with dependency injection targets without claiming a
comparative ranking or SQLite-equivalent reliability. Independent fixtures cover
deadline boundaries, overflow, monotonicity, read/wait failures, cancellation,
repeated PCM/capture/hash results and preservation of actual committed prefixes.
Runtime processing-clock profiling is disabled in the new integration fixtures.
They still use the existing player/presentation/diagnostic-output boundaries;
only the control helper is entirely pure. Execution and platform/performance
acceptance remain deferred. Full layer separation and composition-root-only
concrete selection are unfinished.

Scoped Rust formatting and diff whitespace checks completed. Compile-only
checks succeeded for workspace/all-targets with WebTransport, no-default
WebTransport/all-targets, WASM browser and WASM browser-audio configurations.
Existing dead-code warnings remain. The six new fixture groups were compiled
but not executed; formal review, browser/desktop QA and performance measurements
have not run, and the full player task remains open.
