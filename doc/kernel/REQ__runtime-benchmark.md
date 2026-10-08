# Portable runtime CPU benchmark

`cargo run --release -p beatkernel --example runtime_bench -- --iterations 2000`
runs a bounded software workload, separate from native hardware benchmarks.
The example builds a real chart, canonical keyboard samples with device/native
provenance, explicit native-to-host and host-to-output clock mappings, BindingMap,
Transport, JudgeEngine, scalar command queue, preloaded PCM bank and Mixer.
No default device is opened and no hardware result is fabricated.

All command-line sizes are validated before allocating: measured iterations
1..10000, warmup iterations 0..1000, maximum render frames 8..4096, queue capacity
1..65536, retained timing samples 1..65536, and sample rate 8000..192000 Hz.
A fixed varying block-size pattern, alternating keyboard sources and logical
controls, repeat/up transitions, unbound controls, finite sample/gain variation,
and intentional occasional outside-window inputs exercise composition paths.
Clock timestamps and output scheduling remain checked integer nanoseconds.

Warmup uses the same owners/workload before measurement. Timing retention is
reset after warmup; reported throughput excludes configuration/compilation and
warmup. RuntimeCounters and AudioCounters are reported as measured-phase deltas.
RuntimeTelemetry provides bounded nearest-rank p50/p95/p99/max for measured
Runtime::process_input calls and Mixer::render calls. Instant measures elapsed
software wall time, including clock overhead and any scheduling interruptions;
it is not a hardware cycle counter or exclusive thread CPU-time measurement. Percentiles describe the most recent
retained observations (count/capacity printed), not an unbounded whole-run sample.
CPU throughput includes the complete measured loop, including event construction,
normalization, judging, scalar publication, rendering and PCM checksum retention.
Buffer sizes vary and mixer duration is labeled across those varying sizes.

Mixer work is measured offline, not inside a native device callback. Output frame
counts and PCM checksum prevent treating scalar queue publication as rendering
proof. Queue-full/disconnected, judge results and mixer invalid/late/pending/voice
counters remain visible. Synthetic observed zeros are counts for this software
fixture only. Input-to-hardware latency, physical output latency, native callback
arrival jitter and actual native underruns are printed as unavailable. None is
inferred from CPU duration, silent PCM, sample scheduling, or wall-clock throughput.
The benchmark makes no before/after optimization or guaranteed latency claims.

The 2026-09-30 verification deferral was lifted on 2026-10-06. Local execution
and independent QA are required where feasible. Hardware limitations must remain
explicit and cannot be substituted with software timing claims.

## Versioned software report runner

The example adds `benchmark_schema=1 workload_id=beatkernel-runtime-cpu-v1
debug_assertions=true|false` from its actual compiled assertion configuration.
This banner does not change preparation, warmup, counters or the measured loop.
Changes to fixture inputs, cadence or PCM setup require a new workload identity.

`python3 tools/runtime_benchmark.py --binary <prebuilt-runtime_bench>` runs six
fixed cases: 44100/48000Hz crossed with maximum buffers32/128/512. Defaults are
2000 measured blocks,100 warmup blocks,queue64,timing capacity4096 and3 repeats.
CLI options `--iterations`, `--warmup`, `--repeats` (1..10), `--timeout` (finite
1..240 seconds, default30), `--output` and `--help` are explicit. Existing Rust
iteration/warmup limits apply. The runner never builds or installs the binary.
All child processes execute sequentially with argv, without a shell.

Each result must match the workload/schema tag and requested options. Validate
exact measured frames using `B,B//2,3*B//4,B` at the measured warmup index,
original call counts, finite checksum, retained sample counts and ordered timing
percentiles. Rust output is bounded to64KiB and parsed explicitly without eval;
unknown/duplicate/missing parsed fields, malformed UTF-8 and inconsistent
repeated deterministic values refuse. Zero loop elapsed permits omitted
throughput and records it unavailable. Percentile ordering validation does not
recompute nearest ranks without original duration samples.

The report records schema1, requested settings, case/repetition arguments, raw
stdout, validated software fields, actual debug assertion mode, host OS/machine,
logical CPUs/Python, available Git revision/tracked-dirty state, and artifact
SHA256/byte size. Recheck artifact identity after the full matrix; mutation
refuses publication. Timing/resource observations can vary between repetitions;
deterministic counters, frames and checksum must agree within each case.
Git status collection disables optional locks and must not refresh or rewrite
the repository index. Unavailable/timed-out Git observations remain unavailable.

Parent-observed child wall time and Rust measured-loop elapsed are distinct.
On supported POSIX hosts, one `wait4` owner captures original per-child CPU
user/system seconds and peakRSS. Linux reports KiB normalized tobytes; Darwin
reports bytes. Process metrics cover the whole child lifetime including spawn,
startup, preparation, warmup and reporting, not just measured gameplay. Windows
or unavailable resource APIs use explicit None values; unknown RSS units are
not guessed. No cumulative all-children counter is presented as one run.
See [Python wait APIs](https://docs.python.org/3/library/os.html#os.wait4),
[Linux resource units](https://man7.org/linux/man-pages/man2/getrusage.2.html) and
[Apple's current resource manual](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/man/man2/getrusage.2).

GPU usage and native input/output latency, callback arrival jitter and actual
underruns remain unavailable. Generated synthetic jitter remains labeled as
such. The report supplies no comparative or guaranteed latency claim.

Nonzero exits, timeout, output or observation failure cannot publish a partial
successful report. Terminate only the owned process/group and join it, including
exception paths. Existing output entries (including symlinks) refuse before
launches. Publish complete JSON through a same-parent temporary file and atomic
no-overwrite link; unsupported publication refuses while preserving existing
files. Default stdout publication remains available cross-platform. Temporary
files are cleaned after success or failure.

Independent parser/process/publication tests and an actual release-binary
six-case matrix are required. Debug smoke cannot replace release evidence.
Dense charts, UI stall, multiple players, GPU, long soak and competitor workload
comparison remain WBS13.08 and broader performance acceptance obligations.

## Verified software baseline

Independent CLI QA at `6c62108` rebuilt the release example, passed example
Clippy and 23 report tests, and validated all six default cases three times.
Every run emitted 4785 operations; measured frames were 52000,208000,832000 for
buffers32,128,512, with retained Runtime samples4096 and Mixer samples2000.
The benchmark artifact was854240bytes and reported debug_assertions=false.
This artifact size describes the benchmark executable, not the BMS player.
Actual help/refusal/stdout paths, existing-file and dangling-symlink preservation,
timeout joining, and post-reap descendant cleanup passed.

Evidence is retained locally in `target/wf/qa-cli-runtime-report-01a11d72-1`.
Linux was actually executed. Darwin/unknown-resource/fallback tests do not
establish execution on those hosts. There is no performance threshold,
optimization/comparison, full-player, GPU or physical timing PASS in this result.
