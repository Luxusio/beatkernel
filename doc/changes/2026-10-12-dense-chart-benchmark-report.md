# Dense chart process benchmark reporting

Status: implemented. Independent DEEP code/security review and actual CLI QA
passed. Full WBS13.05/13.08 remain unfinished; this evidence does not grant
receipt-backed task completion or complete the player.

The opt-in Python collector joins the unchanged real dense-chart stress
example to the existing owned-process CPU/RSS and atomic JSON publication
helpers. It fixes two configurations, validates the original nine-line summary
against independent chart/prefix/visibility expectations, and compares
deterministic facts within repetitions of each case. It preserves binary
identity, raw child output, exact arguments and unavailable metric declarations.
The [contract](../kernel/REQ__dense-chart-stress.md) defines admission and scope.

Independent plan review confirmed existing source compatibility and required
separate u128 timing admission, per-case consistency, literal test fixtures,
source/build identity for actual release QA and bounded process-group cleanup
claims. Implementation and independent tests have separate file ownership;
the coordinator alone runs verification after both writers stop.

## Known ceiling

Whole-child CPU/RSS includes startup, chart construction, recording, independent
verification and reporting. It is distinct from the Rust seek/projection phase
timers. Wall time also includes supervisor overhead and its 10ms polling
interval; parent-side count validation is excluded from child resources.
Hash consistency does not authenticate an executable or establish its
source correspondence. Lanes are not multiplayer members, timestamp origin is
not wall-clock soak, and no GPU/audio/native/physical latency or comparative
ranking is inferred. Cleanup covers the owned process/POSIX process group,
not deliberately detached descendants. The complete player Goal stays active.

## Actual verification

Coordinator development checks initially passed43 new/existing benchmark
tests; coverage audit then added oversized decimal admission, and all20 new
tests passed. Independent final QA passed44 new/existing tests in22.608s,
zero failures, exit zero. Fresh release build of the unchanged Rust workload
passed in25.39s using one Cargo job; review/QA source was `87c8ba9`.

The public collector ran outside repository cwd and produced the complete
six-observation report. Two additional real stdout observations and14 CLI
paths passed: help, stdout,9 invalid options, existing output, dangling symlink
and missing binary. Marker checks established refusal before execution and
byte-identical preservation of existing reports. Exact independent facts:

| Case | Holds / records / results | Max visible | Last target ns / cursor / results / visible |
|---|---|---|---|
|20000 notes,8 lanes,16 seeks|5000 /40000 /25000|24|48000000 /200 /128 /24|
|100000 notes,64 lanes,4 seeks|25024 /200000 /125024|128|3000000 /128 /128 /128|

Actual wait4 whole-child peak RSS ranges were81526784..82247680 bytes and
358047744..358187008 bytes respectively. These include the stress workload's
recording, checkpoints and independent verification, and do not measure
whole-player memory. CPU seconds were nonnegative, repeat identities matched,
raw stderr was empty and unavailable metric fields remained explicit nulls.
No speed threshold or cross-player performance ranking was evaluated.

Artifact SHA256:
`b084252e5056db5102fe9bfc79619c3e088378101d716c11bf2ecf5fb77db955`,
size837856 bytes, unchanged. Shared runner/Rust source hashes were unchanged.
Report Git metadata was null under the existing2s inspection fallback; a
separate identity file records actual source/build evidence. Initial verifier
assumptions of nonnull Git and unwrapped argparse help were corrected with
diagnostics preserved; neither required a product change.

Evidence: `target/wf/dense-chart-benchmark-qa-12be8-1/report.json`,
`cli-results.json`, `identity.json`, `tests.log`, `build.log`, and raw command
stdout/stderr. All owned process sessions terminated and the execution lease
was released. Evidence outputs are local ignored artifacts; tests and the
command are committed reproducible verification surfaces.
