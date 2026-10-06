# BMS gauge-rule integration

Integrates the pure adapter rules and nine regression fixtures from
`wf/bms-gauge` (`239a825`) into base `5c7d021`. The application remains one
crate and authored sources remain MIT; no dependency is added.

`BmsChart::gauge_total` resolves retained TOTAL metadata with explicit
Declared/Absent/Invalid provenance. `lr2_gauge_rules` resolves six variants
into fixed-point deltas and thresholds. The copyable state applies judgments
with no allocation or dynamic dispatch. Chart setup counts core judged stages,
including hold head and tail, excluding mines and invisible/BGM events.

This uses the LR2-family numeric tables with explicit BeatKernel stage-count
and integer rounding policies. Recovery truncates per stage and clamps the
retained gauge level; a full PGREAT run need not add exactly TOTAL percent.
See [gauge requirements](../kernel/REQ__bms-gauge.md).

Two additional fixtures cover selected TOTAL headers/duplicates/unchanged
gameplay compilation and maximum public integer inputs/per-stage truncation.
The current gauge suite therefore contains eleven tests.

Verification on this integration (all actual process exits 0):

- Full BMS adapter tests: 87 passed, 0 failed.
- Independently rerun public gauge integration tests: 11 passed, 0 failed.
- Runtime library with webtransport regression: 1,572 passed, 0 failed.
- Workspace all-target check with runtime/webtransport and WASM browser library
  check: passed; existing dead-code warnings remain.
- Independent code/security reviews: scoped PASS with no findings.
- Independent CLI/library QA: scoped PASS at test-suite depth. The public
  parse/chart/rule/state APIs were invoked by integration tests; there is no
  gauge CLI change or separate gauge command.

Ignored evidence logs: `target/wf/gauge-integration-adapter-final.log` and
`target/wf/gauge-integration-qa-cli-{public,runtime,workspace,wasm}.log`.
Actual scoped reviews/QA do not establish whole-task receipt attestation,
desktop/browser/device acceptance or full Goal completion. No task verify or
close is attempted for this unfinished player task.

Actual runtime owners still use their documented default profile. Live variant
selection, opaque-grade mapping, mines and replay/capture policy identity need
application integration before these variants can be used in a recorded game.
Full player/device/browser/network acceptance remains unfinished.
