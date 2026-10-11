# Continuous long-duration virtual player traversal

Status: source `a61b03a` passed independent DEEP code/security review followed
by fresh CLI QA:6/6 integration tests and36 actual command paths, including
complete20h/week and deterministic repeats. Final documentation review and
ordered Harness verify/close remain pending. This advances original WBS13.05
while preserving its scope.

The opt-in `long_duration_player` example starts at song zero and uses the
production audio-authoritative solo pump, Runtime/judge, real Mixer/BgmFeeder,
original input merger, pause/end and replay capture. It renders every successive
sample through complete20h and168h virtual horizons, including idle intervals.
Late origins, seeks or sparse end probes cannot substitute for traversal.
The fixed1000Hz mono grid and sparse hourly notes prove software timeline
correctness; they do not establish standard-rate or physical audio performance.

CLI flags are unique `--hours`1..168(default20), `--block-frames`128..65536
(default8192), or `--help` alone. Malformed, duplicate, missing, unknown or
overflowing values fail before playback-owner allocation. Checked horizon frames
are hours*3600*1000, at most604800000. The combined cold allowance is
2*ceil(horizon_frames/block_frames)+8*hours+64 and must not exceed200000 render
calls plus retained replay operations. Valid individual flags may still exceed
that aggregate budget. Runtime counters also enforce the cap. Capture limits
are200000 records/32MiB encoded file/4096-byte header and bounded input payloads.
Output uses one reusable buffer, queue32, at most8 voices, at most3 one-frame
samples and at most512 checkpoints. Full PCM or every render report is not
retained. Wait/nonprogress loops and independent execution deadlines are finite.

The generated BMS uses BPM60 and measure ratio900: each measure lasts3600s.
Even hourly objects are instant at900s within the hour, released1ms later;
odd objects hold from900s to2700s. Each hour has a BGM cue at1800s. Key and BGM
one-frame samples have literal values0.25 and0.125. Block boundaries split at
original inputs, BGM cues, the pause and terminal fence. An input boundary ends
the preceding rendered range; acquire the original edge and admit its real
Runtime command before rendering the target sample. Positive BGM lookahead
uses the actual feeder; never retimestamp a late cue to manufacture success.

Exactly one owned observation step advances rendering. `render_report()` is a
cached, side-effect-free read because the production pump queries it repeatedly.
Establish coherent native supplied-pair evidence from actual output frames
before acquisition that needs interpolation. Preserve immutable frame basis,
4096 dynamic source slots, history64 and forbidden extrapolation. Observation
age limits derive from the accepted maximum block duration and bounded slack,
without a host-authoritative clock or smoothing policy.

Pause once at horizon midpoint+123ms, away from the authored boundaries, using
real requests, three nonempty held one-frame renders and genuine resume evidence.
Playback-frame coverage equals the full horizon; physical coverage also includes
actual held/drain frames. The runtime fence, Mixer endpoint, NativeEnd,
configuration and capture section end all equal hours*3600s. The last note never
truncates remaining idle playback.

Streaming checks cover every PCM sample and adjacent physical/playback ranges.
Bounded checkpoints are supplementary observations. Verify original source,
sequence/domain/timestamps, instant/hold stages, live judges against genuine
canonical replay and literal expectations, BGM/key PCM before and after pause,
retained Mixer ownership, original terminal evidence and empty command/voice
drain. A failed invariant produces an explicit technical error and no success
summary. Original accepted prefix effects remain inspectable on failure.

Independent tests cover small1h/2h configurations and crossing block sizes,
checked bounds/budget, cached report reads, event-before-frontier chronology,
pause/resume, canonical recordings, late/incomplete acquisition, regressed or
missing native progress, missing pause acknowledgment, host publication refusal
and runtime budget exhaustion. Expected facts do not call workload generators
or oracle helpers. Short tests never replace actual optimized whole20h/week
executions.

Reproduce with
`cargo test -p beatkernel-bms-runtime --no-default-features --locked --test long_duration_player`,
then
`cargo build --release -p beatkernel-bms-runtime --no-default-features --locked --example long_duration_player`
and `target/release/examples/long_duration_player --hours 20` /
`target/release/examples/long_duration_player --hours 168`.
Independent QA records source/artifact identity, exact arguments, stdout/stderr,
exit codes, full coverage counters and repeated deterministic facts. Summary
hashes identify observations, not executable authentication. No full payload or
pointer address is printed; successful summary follows all original invariants.

Independent execution evidence is retained in
`target/wf/long-duration-player-qa-12c37-1/`: exact argv, original stdout/stderr,
exits, per-path facts, source/artifact identity, test/build/format logs and WBS.
QA covered12 successful/help/edge and24 rejected paths, with an independent
literal PCM hash validator. Whole week verified604800003 physical samples,
336 original edges,252 judges,168 BGM samples and empty terminal queues.
See the [development and acceptance record](../changes/2026-10-12-long-duration-player-traversal.md)
for retained failures, remediations and precise evidence scope.

## Known ceiling

Virtual original clock/lifecycle evidence is not hardware acquisition,
native worker retirement, physical sound/latency, standard-rate performance,
GPU/network behavior or real wall-clock soak. Sparse full sample traversal is
distinct from dense stress and64-member coverage. Complete player acceptance,
full13.05/13.08 and the global Goal remain unfinished until their original
requirements are individually proven.
