# Competition progress network policy port

Move common transport notice and progress publication decisions behind a shared
generic port. Native solo/group adapters supply actual readiness, member data and
effects while preserving their distinct cadence/start requirements. Disconnected
comparison state cannot be revived by a late Connected/Ready notice, and network
failure does not undo local gameplay progress.

## Known ceiling

Native acquisition/start/cleanup ownership, group control-clock cadence and some
prefix allocations remain outside this increment. Complete IO separation, actual
network/platform acceptance and performance are not established. Assertions and
formal review/QA remain deferred.

## Compile-only evidence

Both implementation and independent fixture writers stopped before scoped
formatting and checks. All four compile-only configurations completed with exit
0: workspace/all-targets with webtransport; runtime/all-targets with defaults
disabled and webtransport; wasm32 library with browser; wasm32 library with
browser-audio. Existing dead-code warnings remain. Formatting covered only the
six changed Rust files; whitespace checking completed without errors.

Six independent fixture groups are authored, not executed: all notice orders,
first opaque disconnect identity, terminal/readiness monotonicity, all 32
publication gate combinations, refusal with original borrowed rows, and room
observation without bilateral gates. Fixture compilation is not assertion or
runtime evidence. Native transport behavior, later local judge usability,
platform acceptance and latency remain unverified. The full player task stays
open with review/security and browser/CLI/desktop QA still outstanding.
