# Shared live/local stop acknowledgement

Live, local and replay control owners now share acknowledge_batch_with_stop_evidence
around the existing authoritative ACK validator. Solo/local expose the actual
aggregate acknowledged Stop count, readable after a technical failure. Valid full
or partially rejected prefixes retain exact Stop credit; invalid ACK earns none.
Original typed errors and the finite full-success total remain unchanged. The
shared checked evidence overflow uses existing AudioCountOverflow, wrapped by
StepReplay's existing Acknowledgement variant.

The live shared owner now supplies actual Stop ACK evidence to the existing owned
section validator for both finite/unlimited output and to the finite render cursor
validator. Generic checks remain strict and raw counters/frontier checks remain.
Inactive Stop diagnostics can be explained by actual accepted Stops without hiding
other errors or aborting healthy members/BGM solely for numeric failure. Original
failed-member fences, committed prefixes, capture and readiness/drain criteria
remain intact. Numeric-fenced terminal readiness still needs its own integration.

Two independent solo and two local deferred fixture groups were authored using
actual fatal reports, retained batches, remote queues/Mixer, literal PCM and shared
survivor/BGM cases. Existing replay fixtures remain unchanged; two new test modules
share test-only helpers internally. Both writers delivered actual terminal Writes
STOPPED before root scoped rustfmt and all four authorized locked compile-only
checks. Workspace/all-targets webtransport, runtime no-default/all-targets
webtransport, WASM browser lib and WASM browser-audio lib exited zero;
git diff --check emitted no diagnostics. Unused-code warnings include retained
strict generic render/output wrappers and platform cadence/playfield code.

Assertions, real Worklet/browser/device output, performance, formal review and QA
were not executed. Task remains open/PENDING and the whole Goal active; no PASS or
completion receipt is claimed. Numeric-failure terminal readiness, native output
ownership, final clear/fail and high-level mine file admission remain unfinished.
