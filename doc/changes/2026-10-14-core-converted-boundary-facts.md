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
execution. Coordinator no-default-feature library Clippy exits0 with358 app
and111 platform warnings,
not strict-clean status. All139 local links in the five durable documents
resolve; freshness checking invalidates no notes across the20 scoped paths.

The actual independent DEEP review of frozen `7ac7a1af` returns PASS with zero
findings after both discovery attempts. Only then does independent qa-cli run
and return PASS for this bounded DTO ownership and software integration:

| Independent check | Actual result |
|---|---|
| Complete core tests, including doc/integration targets | 746 passed, 0 failed, 0 ignored across79 result targets |
| Platform library | 434 passed, 0 failed, 2 ignored |
| Platform audio format/public-path identity | 23 passed, 0 failed |
| App desktop/WebTransport library | 3260 passed, 0 failed, 8 ignored |
| App binaries | 624 passed, 0 failed, 17 ignored |
| Host binaries, explicit browser-feature WASM | exit0 each |
| Windows/macOS app/platform lib/tests C-stub typing | exit0 each; build-only |
| Feature-enabled app/platform library Clippy | exit0; app394/platform111/core2/BMS5 warnings, not strict-clean |
| Rust15 formatting, diff, WBS and local links | pass;139 link targets |
| Three freshly built public CLI binaries | 12/12 expected stdout/stderr/exit results |

The full suite includes coordinate-epoch13, playback-pause80, live-pause19,
replay-pause4, coordinate-control27, end28, start19, ports38 and owner127.
These filters overlap one another and the larger suites; do not sum them.
The three CLI binaries each exercise help/empty arguments at exit0 and unknown
flag/missing value at exit1. CLI smoke does not execute the DTO; actual library
tests establish that behavior. WASM/foreign checks do not prove browser or
native execution. QA verifies the same20 SHA values, exact scope and tracked
clean tree before and after execution. Documentation audit and owning task
verification/close are separate gates; direct actual PASS results alone do not
supply hook-owned completion evidence or complete the full player Goal.
Local raw logs are under `target/wf/core-converted-boundary-facts/` and are
ignored artifacts, not durable or native-runtime evidence. Independent QA logs
are under its `qa-cli-7ac7a1af/` subdirectory.

## Verification environment incident

The first independent formatter invocation used the cached `cargo/bin/rustfmt`
rustup proxy without `env.sh` or an explicit `RUSTUP_HOME`. It automatically
installed Rust1.98.1's five components under `/home/ccc/.rustup`, violating the
user's no-install constraint. The original auto-install output is in the QA
tool session92450/chunk9feaa1; it was not written to a raw logfile. This is a
reported verification mistake, not a claim that the procedure always complied.

The coordinator checks the new root/toolchain creation times, absence of
settings, only one new toolchain/hash and empty download/temp directories,
then removes only those proven task-created entries and the new empty root.
The independent verifier confirms that root is absent and the preexisting
project cache remains. No source, user default, permission or ownership change
is used for recovery. Permission failures against the preexisting root-owned
target cache are retried with the already permitted narrow sudo; no chmod/chown
or dependency change is needed.

Reusable verified rule: a cached rustup proxy path does not select a cached
Rustup home. Source `target/toolchain/env.sh`, set `RUSTUP_AUTO_INSTALL=0`, and
use the real cached formatter binary. The safe local probe is:

```sh
source target/toolchain/env.sh
export RUSTUP_AUTO_INSTALL=0
"$RUSTUP_HOME/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustfmt" --version
```

The corrected15-file formatter check passes. This cached-proxy gotcha is
captured here as a committed verification rule; no new workflow framework,
installer, provisioning policy or duplicate regression command suite is added.

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
