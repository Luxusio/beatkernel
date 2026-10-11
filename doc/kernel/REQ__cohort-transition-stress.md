# Retained player cohort transition stress

Status: implemented software workload; independent DEEP code/security reviews
and actual CLI QA passed for source1712136. The independent durable outcome
review also passed; ordered Harness verification and task close subsequently
passed for run01a12bfe-28cc-71ab-b21d-f0bd9685d211. Evidence commit:5c31dd7.
This supplements original WBS13.05 and does not complete its full scope.

The opt-in `cohort_transition_stress` example uses actual production solo
and cohort gameplay pumps, original RuntimeGroup/judges/capture policy and
one complete Mixer owner per supported composition. It uses generated
physical inputs and virtual output lifecycle ports with actual PCM/render
receipts; it does not prove hardware clocks, native acquisition or device
retirement. One-member workloads use the existing solo pump;2..64 use the
existing cohort pump. Reimplementing successful practice commits or using a
mock judge does not qualify.

Flags are `--members`1..64(default2), `--cycles`1..64(default8), and
`--notes-per-member`1..64(default8), or `--help` alone. Duplicate, unknown,
missing, malformed and out-of-range flags fail before allocation. Checked
T=members*cycles*notes_per_member must not exceed262144. Admit aggregate
input/capture/PCM/queue/voice/telemetry budgets before constructing owners.
The conservative maximum is166416 render frames, including16 drain frames,
and11174912 retained records across all members. Acquisition closes at every
authored edge or lifecycle boundary and at most every10 idle frames; the
audio authority retains32 observations until real prefix retirement permits
reuse. These are finite software-workload budgets, not player performance
targets.
Cycles mean completed attempts, yielding cycles-1 fresh transitions and
members*cycles original-member archived/final captures. Authored physical
edges are2T; additional Advance/policy repair records are distinct facts.
Original SessionLaunch retry ordinals start at0 and increase contiguously;
the initial recording keeps its base filename and subsequent captures use
`.retry1`, `.retry2`, etc. Generations are a separate one-based sequence.
The final inactive `Ended` receipt advances the control generation to
cycles+1 without creating another active attempt or recording. Active-attempt
generation observations remain1..cycles.

The deterministic source uses 40ms measures. Even-indexed notes are instant
objects at40i+20ms, with physical release at40i+21ms; odd-indexed notes hold
from40i+10ms to40i+30ms. Each section starts at0 and ends at40N+20ms. Thus
N=1 is an instant-only case and larger cases exercise both families. Actual
1000Hz mono Mixer renders advance one frame at a time; literal one-frame key,
section cue and stale sentinel samples have values0.25,0.125 and-0.75.
The independent PCM oracle must account for original command admission and
actual output frames, including pauses, rather than rewriting source timing
to match observed output.

Retained practice checks qualify genuine Mixer receipts, retire original
acquired prefixes, archive each original member capture and apply/publish
fresh attempts through existing production ports. Each attempt preserves
original player/device identity, event metadata, canonical recording, section
and retry lineage. Independent expected replay/judge and literal PCM checks
must cover all members, backward song anchors, stale-scope exclusion and a
final empty command/voice drain. Physical output remains monotonic.

Acknowledged pause/resume and admitted output replacements use actual paused
render observations, immutable frame bases, progressing virtual supplied
clock pairs and existing replacement/publication policy. A local wrapper owns
the same Box<Mixer> allocation through retirement/recovery and reopening;
directly incrementing a pause epoch is insufficient replacement evidence.
Private bank/consumer addresses are not externally inspected or claimed.

Current application policy explicitly refuses output replacement during
retained practice. The workload must test refusal before retirement/mapping
without owner/prefix damage, and exercise successful rebind in its separately
admitted composition. It must not bypass admission through low-level APIs or
claim successful practice/replacement interleaving. This preserves the current
rule; it does not permanently exclude future support from the complete player.

Failure verification covers atomic last-member replacement refusal, late or
incomplete acquisition prefixes, bounded queue/receipt exhaustion, archive or
presentation failure with original committed evidence, unacknowledged pause,
replacement timeout and premature recovery. A failed invariant exits nonzero
without a successful summary. Versioned summaries expose configuration,
authored/captured counts, attempts, identity/PCM/replay and transition checks.
The CLI declares `stress_schema=1` and
`workload_id=beatkernel-cohort-transition-v1`. It emits one association per
active attempt and original-member capture, retaining original path, player,
device, replay byte count and judge hash. Replay bytes and PCM are summarized
with FNV-1a-64; PCM feeds the little-endian bytes of each f32 bit pattern.
These hashes are deterministic identity observations, not authentication.
Pointer addresses and full PCM/replay payloads are not printed. Successful
output follows workload invariants and an additional lineage/association/
pause/queue/replacement summary check; failed checks produce no success banner.

Reproduce after implementation with
`cargo test -p beatkernel-bms-runtime --no-default-features --locked --test cohort_transition_stress`
and
`cargo run --release -p beatkernel-bms-runtime --no-default-features --locked --example cohort_transition_stress -- --members 64 --cycles 8`.
Actual CLI QA includes default,1/2/3/4/64 members, repeated attempts and
invalid/edge paths under sequential finite resource limits. Independent tests
may call implementation-under-test, but expected facts must not derive from
its generators/oracle helpers. Hardware/GPU/network, real64 remote hosts,
comparative speed and full wall-clock soak remain separate original obligations.

## Actual software evidence

Independent QA freshly passed11 integration tests, scoped formatting, an
optimized build and30 CLI paths:12 help/happy/edge executions and18 invalid
inputs. The member matrix covers1/2/3/4/64, repeated64 output is byte-identical,
and cycles64/notes64 edges are separate bounded configurations rather than an
exhaustive Cartesian product. The64-member3-attempt3-note case preserves192
original captures,1152 authored/captured input edges and2 fresh transitions.
Both independent defect-hunter inputs were empty; formal code and security
reviews returned PASS with no findings before QA began.

Raw results and source/build identity are in
`target/wf/cohort-transition-stress-qa-12bfe-1/`; the ignored local directory is
not committed or portable. The optimized artifact is3236848 bytes with SHA256
`da9ddf6fc9b0042d02db02e5dc7ad73d9489ec8a94f00c8d37eb7d0476e48b28`,
verified unchanged throughout those executions. Source1712136 and original
stdout/stderr are preserved in that identity record. The
[change record](../changes/2026-10-12-cohort-transition-stress.md) retains failed
development attempts, the private compiler recipe and measurement ceilings.
Original WBS13.05/13.08 and complete player acceptance remain unfinished.
