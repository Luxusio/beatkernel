# Gameplay presentation injection

Shared solo and local-cohort gameplay previously accepted a concrete platform
PresentationDiscipline and constructed that type at resume. The actual loops
now use business-owned GameplayDevice and GameplayPresentationPort contracts,
with static dispatch linking a device to its injected presentation owner. The
pure core PresentationEstimator implements the port. Resume construction uses
explicit stream/playback/host/song origins before the device's original reseed.

The outer native compatibility bridge owns the unchanged NativeGameplayDevice
contract, platform port implementation and native session specializations. Its
blanket adapter preserves original native errors and acquisition methods. Native
launchers and old fixtures retain their public call shapes and assertions. No
new crate or dependency was introduced. New independent deferred fixtures use
core-only presentation and injected controls/hosts in the actual gameplay pumps.

Both writers stopped before scoped rustfmt and git diff --check. All four
root compile-only checks exited zero: workspace/all-targets with webtransport;
runtime/all-targets without defaults with webtransport; WASM browser library;
WASM browser-audio library. Existing dead-code warnings and a new test-only
ClockPair import warning remain. Assertions, formal review/QA, real devices
and benchmarks remain unexecuted.

Eight independent deferred fixture groups were authored: three pure port
contracts, three actual solo pumps, and two actual cohort pumps. Actual solo
pause/resume and distinct playback-origin reconstruction are separate cases;
the combined distinct-origin pause/resume pump scenario is not yet covered.
Compilation does not establish assertion success.

## Known ceiling

전체 레이어 분리와 실제 플랫폼 검증은 미완료 — 후속 경계 분리 및 유예된 검증에서 확인.

Concrete competition adapters still mix network, clocks and publication; native
processing telemetry defaults and further IO boundaries remain to be separated.
No full-player completion, comparative performance or SQLite-grade reliability
is established by this increment.
