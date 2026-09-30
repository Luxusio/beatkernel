# Chronological offline BMS rendering

The separate BMS runtime crate now shares asset preparation between its offline
and native composition roots. Offline rendering advances synthetic input, BGM
and actual Runtime/JudgeEngine/Mixer execution on the same integer output frame
grid, rendering between scheduled frame groups. Total chart notes no longer
determine concurrent voice or command storage capacities. Finite same-frame
command/voice failures are explicit rather than silent drops. Output remains
newly created interleaved f32le in bounded chunks; requested duration bounds
both sound execution and synthetic judging. Output channels default to two with
an optional explicit channel argument. WAV remains the default decoder and no
compressed format support was added. Tests and formal/native verification remain
deferred by the user; the full original Goal remains active.

The game-independent Runtime also exposes explicit control-thread audio command
admission to its existing producer. Background commands and judged keysounds use
one queue and the same admission counters; output-domain timestamps are supplied
by the caller. This operation does not alter gameplay, transport or input
chronology, and queue success remains separate from actual mixer execution.

Rust 1.98.1 locked compile-only checks passed for the complete workspace/all
targets on the Linux host, plus platform/BMS runtime all targets for
`x86_64-apple-darwin` and `x86_64-pc-windows-gnu`. Eight independent offline
fixtures and two core admission regressions were authored and compiled, not
executed. They cover literal timing/PCM, chunk partition invariance, 5000 sparse
notes using one voice/command slot, cutoff/ceil boundaries, capacity/execution
errors, overflow and writer failure. Builds provide source compatibility
evidence; they do not establish fixture results, native playback or acoustic
synchronization. Apple ARM64 compilation was not performed.
