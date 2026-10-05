# Common BMS gauge observations

Gauge state belongs to the BMS runtime and consumes actual committed judge and
hazard reports. It does not reinterpret opaque grade numbers, judge inputs,
emit sounds, rewrite scores or infer new input. Use one shared BmsGauge policy
for stepped solo/local playback and incremental replay presentation.

## Fixed-point policy

One percentage point is 1,000,000 integer units; maximum level is 100,000,000.
GaugeProfile specifies initial/clear levels, signed default hit and miss deltas,
an optional fail-on-empty latch, and up to 64 explicit opaque grade overrides.
Validate initial/clear bounds, override capacity and duplicate grades at setup.
Sort overrides once; hot observation uses binary lookup and no allocation.
Signed deltas use wide arithmetic and clamp to [0, maximum], including i64
extremes. An unlisted hit uses the explicit fallback delta, not grade ordering.
Every actual normal judged stage contributes once, including hold head/tail;
invisible presses and mine markers contribute no normal-note stage recovery.

The documented BeatKernel default is initial 20%, clear threshold 80%, +1% per
successful judged stage, -6% per missed stage, and recoverable zero. It is an
explicit application policy, not LR2/IIDX/Angolmois gauge compatibility. TOTAL
remains preserved source metadata in this slice. Pure custom profiles support
other grade deltas and fail-on-empty policy; actual session owners use the fixed
documented default until configurable session policy and capture identity are
connected. Do not silently expose configurable live policy with an unrecorded
replay configuration.

Observe the report's normal results first, then its hazard events in original
order. Validate every hazard value in 1..1295, including avoided outcomes and
events after failure, before committing a batch. Nonfatal triggered values cost
exactly value * 500,000 units; clamp damage exceeding the whole level to zero.
Avoided events cost nothing. Triggered 1295 (ZZ) sets level zero and latches
InstantDeath independently of fail-on-empty. These mine meanings follow the
[original Angolmois documentation](https://github.com/lifthrasiir/angolmois/blob/master/INTERNALS.md#data-commands);
the full nonfatal range remains BeatKernel's explicit policy. Empty failure
latches Depleted; retain the first failure reason and freeze subsequent numeric
changes. An initial zero profile with fail-on-empty is initially Depleted.
Invalid report batches leave the previous gauge snapshot exactly unchanged.
Clear readiness is a level/failure predicate, not evidence that a song finished.

## Actual owners and remaining terminal control

StepGameplay and each StepLocalGameplay member own a default BmsGauge and
consume actual committed report prefixes even when judge/audio/score/capture
postprocessing also fails. Retain the original report and independent errors
when gauge aggregation fails, fence technical processing and expose the previous
atomic gauge state. Numeric gauge failure is readable game state; it is not a
technical error and does not discard committed audio or falsely mark output as
drained. Do not fold independent local gauges into one cohort gauge.

ReplayVisual observes normal and hazard results immediately after each actual
recorded operation. Repeated/equal display targets, time beyond a recorded
prefix and unrecorded display advancement produce no extra gauge changes.
StepReplay delegates to the same gauge rather than observing results twice.
Keep existing replay setup bytes and core judge identity unchanged; this slice
derives the documented fixed application policy from the same recorded results.

Native gauge publication, graphics/HUD, completion clear/fail decisions and
actual per-player failure fencing/output cleanup still need integration.
InstantDeath state alone does not establish playback termination. Keep the
high-level mine file admission guard until those owners are connected. Author
independent deferred fixed-point/configuration/atomicity and real solo/local/
replay prefix/failure fixtures; no application, browser, device or performance
acceptance is established by source compilation.
