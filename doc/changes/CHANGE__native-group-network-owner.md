# Native group network owner

GroupMultiplayer is a typed facade over the existing native network owner. It
uses one prepared common Session and the same QUIC/WebTransport endpoint,
worker, clock epoch, readiness, start and full-write/final-ACK cleanup path.
Bounded ordered member snapshots and typed group/lifecycle events share the
existing application queues. Scalar APIs remain available without converting
group scores into a scalar peer. Admission failures preserve prior prefixes;
ordinary saturation fences, while terminal saturation is retryable within the
cleanup deadline.

Known ceiling: Native application/CLI/GUI callers still require migration to
the group facade before connecting to the Page's group mode. Multi-host rooms,
actual socket/device/audio/browser/generated-binding behavior, performance and
formal review/QA acceptance remain unfinished. Fixtures are authored for later
execution under the standing deferral; compilation cannot prove transport
acceptance or full player completion.

Six independent deferred fixture groups cover constructor/preflight admission,
whole-prefix atomicity, ordinary/terminal queue saturation, retained typed
results, scalar compatibility and actual Session full-write/final-ACK boundaries.
Existing embedded scalar assertions retain their meaning with typed channel
plumbing. Fixtures were not executed.

Scoped rustfmt and whitespace checks completed. With Rust 1.98.1, all four
authorized Cargo check configurations completed with exit code 0: workspace
all-targets, runtime no-default-features all-targets, and wasm32 library checks
for browser and browser-audio. Existing wasm32 platform cadence dead-code
warnings remain. These are compilation checks, not runtime or QA evidence.
