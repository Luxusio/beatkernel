# BeatKernel Phase 0/1: time and transport

The initial workspace contains exactly `beatkernel` and `beatkernel-platform`.
Only the platform crate depends on the core. OS modules are cfg-gated stubs;
native input/audio, chart compilation and judgment are later phases.

## Integer time

`Timestamp` and signed `Duration` store i64 nanoseconds. Constructors and
accessors preserve exact values, including negative song positions. Checked
arithmetic returns `None` on overflow rather than wrapping, saturating, or
changing behavior between debug and release. Clock points carry explicit domain
IDs. `ClockMapper` returns `None` for an unsupported or unrepresentable mapping;
mapping quality distinguishes exact, estimated (nonnegative uncertainty), and
unknown relations. Distinct domains must never be treated as interchangeable.

## Rates and mapping

`Rate` stores a signed i64 numerator and positive u64 denominator. Construction
rejects zero denominator and reduces equivalent fractions, including canonical
zero `0/1`. MIN numerator is supported. Multiplication precedes division in i128,
with integer division truncating toward zero; no floating point is canonical.

`Transport` begins with one host/song/rate anchor. Mapping selects the latest
anchor at or before a host time, including the last command if several anchors
share a host timestamp, and computes:

```text
song = anchor.song + trunc((host - anchor.host) * numerator / denominator)
```

Host subtraction, scaling and song addition use i128. Only the final mapped song
timestamp is checked against i64. Queries before the first anchor return
`BeforeOrigin`; an unrepresentable final position returns `Overflow`.

## Commands

Commands take caller-supplied host time from one normalized monotonic domain.
Times may be equal, but cannot precede the last successful command, including a
no-op; backward commands return `NonMonotonicHost`. Errors leave all state intact.
Read-only historical queries do not affect command chronology.

A true rate change anchors at the old mapping's current position, preserving
continuity. Zero rate pauses. Pause remembers the last nonzero rate; resume
restores it, including reverse rates. A transport constructed at zero initially
resumes at 1x. Setting a nonzero rate while paused starts that rate immediately.
Seek sets the requested song position while retaining active rate and remembered
resume rate, and can recover from an overflowing old trajectory. Reverse playback
permits negative song positions and never clamps at zero.

Setting the current rate, pausing while paused, or resuming while playing validates
the command and advances its chronology without creating an anchor. These no-ops
preserve fractional progression. Real changes quantize the new anchor to integer
nanoseconds; discarded fractional values do not carry across true changes.

## Execution and verification

History is retained for late timestamped inputs: lookup is O(log n), append is
amortized O(1), memory is O(n). Mutations can allocate and belong on a control
thread, outside the future real-time audio callback; position lookup does not
allocate. No unsafe code or platform API is used in Phase 0/1.

Tests cover normal/half/double/reverse rates, pause/resume/seek, same-time ordering,
historical lookup, wide host spans, fractional no-op behavior, overflow and failed
mutation atomicity. Deterministic property sequences apply at least 1,000 rate
changes each against an independent incremental oracle. Debug/release tests,
fmt, clippy, doctests, a runnable example and workspace dependency inspection
are required. The CI matrix targets Linux, Windows and macOS; local verification
only claims targets actually executed.

## Known ceiling

Known ceiling: Platform modules are metadata stubs without native I/O — upgrade in the native backend phases.
