# Dense chart and repeated seek software stress

The opt-in `dense_chart_stress` example exercises real JudgeEngine,
ReplaySession and VisualProjector implementations with deterministic mixed
instant/hold chords. It supplements the unchanged runtime/audio v1 benchmark.
Its workload identity is `beatkernel-dense-chart-v1`, schema1, and it reports
the binary's assertion mode. No replacement judge or mock projection qualifies.

## Configuration and admission

CLI flags are `--notes`, `--lanes`, `--seek-cycles`, `--origin-ns` and `--help`.
Defaults are20000 notes,8 lanes,16 seek checks and origin0. Limits are
1..100000 notes,1..64 lanes,1..1000 checks and0..604800000000000 origin ns.
Checked `notes * (seek_cycles + 1)` must not exceed5000000 before fixture
allocation. A cycle is exactly one seek, fresh reconstruction and projection
check. The library-independent workload entry validates the same constraints.
Unknown, duplicate, missing, malformed and out-of-range flags return nonzero
without a success summary. Values are neither clamped nor silently replaced.

## Exact chart and recording

Object index i has IDi+1, row i/lanes and lane i%lanes. Row heads occur every
4ms after origin. Row%4==0 is a hold ending2ms after its head, including row0;
other rows contain instant notes. A partial last row preserves the same rule.
Separate press evaluators per lane/family use zero timing windows and offset.
Each row records all Down events at its head followed by all owning Up events
at head+2ms, including instant releases. This yields exactly2*notes input
records with ordered timestamps/ordinals and preserved physical/native metadata.

Complete recording must match an independently constructed golden sequence
of IDs, stages, exact times, grade1, zero delta, controls and original input
metadata. Expected results are notes+hold_count; no miss, pending object or
held owner remains. Count equality alone is insufficient proof.

## Repeated reconstruction and projection

Retain at most origin/mid/end normal checkpoints. Each seek's cursor, full
result prefix and judge hash must equal a fresh JudgeEngine applying the
unchanged input-record prefix through the target. The oracle uses no seek or
checkpoint restoration. It advances to the target only if the prefix is empty
or the target exceeds the final included record time, matching boundary meaning.

Checks rotate head, mid-hold, tail, gap and final times. Representative hold
rows vary between groups; final-to-head transitions exercise backward seeks.
For cycle c, hold_row=((c/5)%ceil(rows/4))*4. Slots0..3 use that row's
head+0/1/2/3ms; slot4 uses the last row's head+3ms.

The actual indexed lane projector uses unit1ms and window target±4ms, except
tail slot2 uses target±0.5ms. The tail window excludes its hold head while
including the tail. Independent overlap is inclusive:
start<=window_end and end_or_start>=window_start. Compare ordered IDs, lanes,
head/tail distances and finiteness, not merely visible counts. RenderFrame
capacity4*lanes and its storage pointer must remain unchanged during queries.
Transient events are empty for these projection measurements.

## Measurements and limits

Successful stdout contains options, hold/record/result/checkpoint counts,
seek/projection check counts, maximum visible objects and final judge hash.
Setup, recording, seek, projection and verification nanoseconds are separate.
Independent reconstruction, golden scans, hash comparisons and probe copies
belong outside measured seek/projection calls. Timings are observations with
no performance threshold, ranking or universal zero-cost claim.

The fixture exposes bounded generated records/chart and probe facts to
independent integration tests. It writes no files and starts no threads or
child processes. CLI QA directly executes release help/default/edge/refusal
paths under time/address-space limits with one compiler and no simultaneous
workload execution.

20-hour and one-week origins test absolute timestamp placement only. They do
not prove long-duration charts or actual wall-clock soak.64 lanes are not64
multiplayer members. Audio, native clocks/latency, GPU rendering, physical
devices, whole-player behavior and broad soak/stress acceptance remain required
elsewhere. WBS13.05 and13.08 remain W; this child does not complete either leaf.

Reproduce with `cargo test -p beatkernel --locked --test dense_chart_stress`,
`cargo build --release -p beatkernel --example dense_chart_stress --locked`,
then the built example with default options or `--help`. Exact executed QA
results and evidence paths are recorded after independent verification.

## Verified implementation

Frozen source `891eb85` passed independent DEEP code, security and documentation
review followed by CLI QA. Evidence is in
`target/wf/qa-cli-dense-chart-01a11dd7/REPORT.md` and `cli-results.json`.
Full core tests:426 passed, zero failures, including9 independent dense tests.
All-target core Clippy, scoped formatting and the release example build passed.

QA executed38 release CLI cases: help, default, minimum, partial rows,64 lanes,
100000 notes, maximum check count, repeated20h/week timestamp origins and28
invalid inputs. All invalid inputs exited1 without success stdout; successful
summaries matched independent count, target, overlap and probe formulas.
Repeated command facts matched excluding informational timing fields. All
compiler/workload processes reached terminal status; no timeout was claimed
as proof. Cargo used one job,240s cap and8GiB address-space limit; workloads
ran sequentially with90s cap and8GiB address-space limit.

Default20000 notes/8 lanes/16 checks produced40000 records,25000 results,
3 checkpoints and maximum24 visible objects. The100000-note/64-lane/4-check
case produced200000 records,125024 results,3 checkpoints and maximum128
visible objects for its scheduled windows. This is not a worst-case capacity
or performance guarantee. The earlier runtime v1 source remains byte-identical
to base `9ff3ca1`. Whole13.05/13.08 and full-player acceptance remain open.
