# Continuous long-duration virtual player traversal

Status: source `a61b03a` passed independent DEEP code/security review followed
by CLI QA. Fresh independent integration6/6 and36 actual CLI paths passed,
including complete20h/week and deterministic repeats. Both earlier review
findings are corrected. Final document review and ordered Harness verify/close
remain pending; full player Goal and original WBS13.05 are unfinished.

The [contract](../kernel/REQ__long-duration-player-traversal.md) requires actual
production audio-authoritative playback from song zero through every output
sample of20h and168h virtual horizons. It uses sparse hourly BMS objects/BGM,
a fixed1000Hz software grid, real Mixer/Runtime/judge/capture and genuine
pause/end evidence. Streaming PCM verification and bounded checkpoints avoid
retaining every sample/report. A late origin, seek or endpoint probe cannot
replace continuous traversal.

Independent source-backed planning verified the public solo audio pump,
4096-slot dynamic source admission, BMS measure-ratio/event timing, repeated
cached-report queries, original playback-frame BGM cursor and incremental
capture encoding. It clarified combined render/capture budgets, exact finite
endpoint alignment and command admission before target-sample rendering.
Source and independent test paths have distinct owners; the coordinator alone
runs compilation and execution after both writers freeze.

This child also reconciles the previous cohort contract/change status snapshot:
source1712136 and evidence5c31dd7 subsequently passed independent final docs,
ordered Harness verification and task close. No prior source or QA is changed.

## Development evidence

Original logs remain in `target/wf/long-duration-player-development-12c37/`.
`first.log` ended with compile exit101 because the solo host omitted the required
`publish_local_reports` method. Its implementation now explicitly refuses
cohort reports. `fixed-api.log` compiled but passed2/6 tests: snapshots incorrectly
recaptured the Mixer's current physical frame as a fresh stream creation basis.
The adapter now retains its original creation basis while supplied pairs still
use actual advancing physical frames. `fixed-basis.log` passed3/6, including
all original-port fault checks; complete traversals reached an endpoint error
because the pause owner lacked finite-endpoint setup. That setup was corrected
through the existing public pause API using the original common endpoint.
`fixed-endpoint.log` subsequently passed6/6 in1.74s after16.34s compilation,
including four complete1h/2h traversals with128/8191/65536-frame blocks and the
seven injected original-port failures. These are development outcomes,
not independent acceptance evidence; test expectations and production validation
conditions have not been weakened.

The release build completed in14.09s. The actual default20h command verified
72000000 playback frames and72000003 physical samples,8814 renders/8812 replay
records,40 original edges and30 original/reconstructed judges. The168h command
verified604800000 playback frames and604800003 physical samples,74008 renders/
74006 records,336 original edges and252 judges. Both retained three genuine
held renders, their original Mixer allocation and empty terminal input/command/
voice queues. Week PCM FNV hash is11253828116591174805; canonical replay FNV is
10144961940988229877 with1280827 encoded bytes. Two complete week executions
produced byte-identical stdout. These hashes identify observations, not trust.

Nine actual development CLI cases passed: help, default20h, week, repeated week,
1h/128-frame minimum block, aggregate over-budget, duplicate flag, numeric
overflow and missing value. Four refusal paths returned1 with empty stdout.
Exact argv/exit/time and original stdout/stderr are retained in `cli-results.json`
and named artifacts in the same directory. The default and week observations
took approximately0.93s and7.09s respectively on this host; these are virtual
correctness execution times, not standard-rate player or hardware performance.

Independent discovery cycle1 returned one correctness candidate and no contract/
test candidates. Formal code review verified the candidate as a medium missing
behavior: the CLI reconstructed the original in-memory `ReplayFile` instead of
decoding its encoded capture bytes. Small independent tests already exercised
decode separately, but full-horizon CLI acceptance requires its own actual
encode/decode/reconstruction path. The correction uses existing `decode_replay`
with the same bounded limits and reconstructs its returned file. Review1 is
FAIL, not acceptance; its earlier whole-horizon observations are preserved.

After that correction, `fixed-decode-test.log` passed6/6 in1.63s and
`fixed-decode-build.log` completed the optimized build in12.87s. The actual
20h,168h and repeated168h commands each passed through capture byte decoding
and completed all frames. `decoded-cli-results.json` and `decoded-*.stdout`/
stderr retain those original executions. Both week outputs were byte-identical,
and20h/week facts also matched the corresponding earlier outputs. No recording
format, timestamp, expected PCM or test oracle changed.

Discovery cycle2 returned no correctness candidates and one contract/test
candidate. Formal review2 verified the remaining compiled BGM connection as a
medium missing boundary: actual BGM commands and expected PCM were derived
from the same hourly literals, leaving `compiled.bgm` unused by long playback.
Actual commands must instead retain each original compiled cue's sample/time;
the complete compiled schedule must match independent hourly expectations while
the streaming PCM oracle stays separate. Review2 is FAIL. Discovery's two-cycle
budget is exhausted; after correction a fresh formal full-scope reviewer will
verify both remediations without further hunters.

Actual BGM commands now use every compiled cue's original sample and timestamp;
all hourly cue metadata is checked against separate literal expectations before
playback. The independent test also pins `SampleId(2)` for each compiled cue.
`fixed-bgm-test.log` passed6/6 in1.58s; `fixed-bgm-build.log` completed in12.93s.
Actual20h/week/repeated-week executions passed and matched the earlier expected
stdout byte for byte. Original outputs, exact argv and exits are retained as
`compiled-bgm-*.stdout`/stderr and `compiled-bgm-cli-results.json`. Neither PCM
expectations nor the original input/timing/hold-tail workload policy changed.

## Independent acceptance evidence

Final code review3 and security review3 passed with zero findings at
`a61b03ae73713a3533f59fcf157751b7ea0884ca`; code review retained DEEP and used
the formal-only exhausted discovery path to inspect the complete eight-file
change and verify both prior remediations. Actual CLI QA then passed all four
roles and6/6 fresh integration tests (6.22s compilation,1.53s execution), scoped
formatting, an optimized build and36 direct commands:12 successful/help/edge
paths and24 invalid paths. No source changed during QA.

QA executed default20h, explicit/repeated20h, complete/repeated168h and
168h/65536;1h blocks128/8191/65536; odd3h; reversed flag order and help.
Rejected paths covered both bounds, aggregate budget, duplicate/missing/unknown
flags, positional/equals/help combinations, overflow, empty, signed, whitespace,
Unicode and nondecimal values. All refusals returned1 with diagnostics and no
stdout. Independent Python literal PCM hashes matched every successful case;
default/explicit/repeated20h and repeated168h outputs matched byte for byte.

Week counts remained604800000 playback/604800003 physical samples,336 original
edges,252 original/reconstructed judges,168 BGM samples,148014 combined render/
record operations and297 checkpoints. Three genuine held frames, zero repair
records and empty terminal input/command/voice queues were verified. The focused
suite also checked canonical encoded-byte decoding/reconstruction, literal
original metadata and seven genuine original-port failure scenarios.

Raw evidence is in `target/wf/long-duration-player-qa-12c37-1/`: original test/
build/format logs, exact command argv/exits/times and per-path stdout/stderr,
`run_matrix.py`, `cli-results.json`, `identity.json` and `wbs.json`. The accepted
optimized artifact is2648896 bytes, SHA256
`6b48842d1ce6b37de0ea8f2f62d6ccaa9e8600c204b6bc04ff1ae4b0ce66356d`.
All QA sessions53878/31589/21209 finished with exit0; command leases released.
WBS remains91/193,47.15%,102 unfinished and4 conditional. This evidence closes
the child's executable software checks, not original13.05/13.08 or the Goal.

## Known ceiling

Sparse1000Hz virtual timeline correctness does not prove standard-rate audio
performance, physical sound/latency, native acquisition/worker retirement,
GPU/network, actual wall-clock soak or full-player resource stability. Existing
dense/64-member evidence remains distinct; full13.05/13.08 and the player Goal
retain their original completion requirements.
