# Core ownership of converted boundary facts

`ConvertedBoundaryFacts` now belongs to existing `beatkernel::audio`, beside
`TargetBoundary`. The platform audio namespace directly re-exports that same
type, and ten shared application/port consumers name the core path. Public
fields, documentation, derives and the default absence sentinel are preserved.
No wrapper, conversion, dependency, runtime operation, allocation or dispatch
layer is added. Native state, render/admission/retirement, telemetry and
presentation validation retain their existing owners and behavior.

The contract is in [target time](../kernel/REQ__native-target-time.md) and the
ownership decision in [native boundaries](../kernel/ADR__native-runtime-boundaries.md).
Both public paths name the same nominal type within the new build. Its defining
reflection path changes; stable Rust ABI/layout, cross-build type identity and
serialization compatibility are not promised. Generated boundaries remain
distinct from native submission or audible presentation acknowledgments.

## Verification

All fourteen source files and the independent test-author's existing integration
test file are frozen before shared builds. The new test uses explicit core to
platform to core assignments without conversion. Actual foundation checks pass:
core-only library check, core boundary7/0 and converted mixer16/0, platform
library434/0/ignored2 and audio format23/0. Existing platform-path native and
fixture consumers remain to exercise source compatibility. Direct `pub use` is
also a source-review requirement because typed assignment accepts Rust aliases.

All fifteen owned Rust files pass cached edition2021 formatting with
`skip_children=true`; diff checks pass. Only the four enumerated baseline
declaration-format corrections are included, with executable statements and
assertions preserved. App `target_` passes327/0/ignored1. Host binaries and
explicit browser-feature WASM check exit0. Windows/macOS app/platform lib/tests
C-stub Rust typing also exit0; this proves neither native ABI nor device
execution. Non-denied Clippy exits0 with358 app and111 platform warnings,
not strict-clean status. All139 local links in the five durable documents
resolve; freshness checking invalidates no notes across the20 scoped paths.
Independent formal review followed by independent full application QA, and
the final documentation audit, are still pending.
Local raw logs are under `target/wf/core-converted-boundary-facts/` and are
ignored artifacts, not durable or native-runtime evidence.

## Remaining scope

[WBS01.05/01.06](../kernel/REQ__bms-player-work-breakdown.md) remain W. This
removes one outward pure-value dependency; application still depends on concrete
platform output state and presentation models, including some backend evidence
interpretation. It does not finish all contexts, effect injection or native
resource composition. No measured speedup or whole-player zero-cost claim follows.
Current progress remains106/223(47.53%); original product requirements remain
91/193(47.15%). Native device/backend acceptance, browser execution, multiplayer,
physical timing/performance, stability and release requirements remain active
in the full BMS player Goal. This task introduces no new native effect or UI.
