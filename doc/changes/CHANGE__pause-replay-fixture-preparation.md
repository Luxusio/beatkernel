# Restore meaningful pause, replacement and replay fixture preparation

The live pause helper used a 128-byte per-asset cap for 64 f32 samples (256
bytes); the interval pause helper used 256 bytes for 128 f32 samples (512
bytes). Their 11 tests failed at PcmSample construction, before reaching clock,
transport, pause/resume or capture assertions. Set each cap to the actual byte
extent. Bank budgets and behavior assertions remain unchanged.

Two output replacement tests expected a mixed value of 1.125, contradicting the
required final clamp to [-1,1]. Merely expecting clipped 1.0 would hide whether
the queued second voice was admitted. Give that voice gain -0.5 and expect 0.875
instead, retaining an observable contribution after successful replacement or
recovered failed open. Clock, epoch, hold and original error assertions remain.

The mine audio and fatal gauge replay fixtures loop over ButtonOnly and
ButtonOrContact. Contact captures use v5 metadata, which the legacy plan_audio
entrypoint deliberately refuses. These mode-aware reconstruction cases now use
plan_section_audio with the same arguments. Existing command order, judge hash,
PCM, recorded extent, fatal stop and missing WAV00 assertions are retained.
Other legacy planner calls, including refusal cases, remain unchanged. No decoder
or production admission rule is weakened.

This increment changes fixture setup only. It adds execution coverage of the
existing common pause and replay logic; it does not establish physical output
latency, real device switching or exhaustive synchronization correctness. The
broad task remains open with independent review and required QA pending.

Verification (2026-10-06): full runtime library with webtransport reported
1494 passed, 74 failed, compared with 1479/89 immediately before this change.
There are no new failing names. The resolved 15 comprise live pause (4), interval
pause (7), output replacement (2), mine replay audio (1) and fatal gauge replay
audio (1). All changed fixtures were compiled and executed in that full test run.
The full suite still exits 101 for the 74 remaining failures; this is not a task
or whole-project PASS.
