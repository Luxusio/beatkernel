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

Building/formatting the example is allowed during current implementation. Running
the example, formal QA, benchmarks and native hardware checks remain deferred by
user instruction on 2026-09-30.
