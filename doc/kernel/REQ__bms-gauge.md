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

## Native publication contract

Each local presentation member retains its own default BmsGauge. Native solo
and group report publication updates gauge, score, mine summary, pressed state
and history atomically after whole-batch validation. An invalid later member
must leave every earlier member unchanged. Empty deadline reports do not clone
gauge profiles or allocate gauge scratch. The legacy solo gauge mirrors exactly
one member; a multi-member cohort has no aggregate gauge.

Actual replay presentation copies the authoritative ReplayVisual gauge, including
pause-boundary publication. It must not rebuild gauge order from cumulative mine
damage or apply incremental judgments a second time. Only the documented default
profile is admitted by this bridge until capture policy identity is connected.
An unchanged absolute replay gauge remains unchanged on repeated publication.
Legacy replay APIs lacking an authoritative gauge cannot establish mine-aware
gauge accuracy: they observe actual normal judgments without synthesizing mine
events from a cumulative summary. Actual replay callers must use the full gauge
publication API. Reject a default replay gauge whose instant-death state disagrees
with its summary, or which changes an already frozen failed state, before any
presentation mutation. Ordinary level loss/recovery is not a monotonic counter.

These retained gauges are presentation state. Unattached native publication
remains a no-op; headless termination policy must live in the actual game owner,
not depend on whether a UI publisher is installed.

## Common graphics HUD contract

Native solo/local and browser Worker solo/local/replay render the same borrowed
gauge state through a small common drawing component. Render a bounded bar and
integer-derived percentage truncated to two decimals. READY denotes the current
clear threshold predicate only; it must not claim the song has cleared. DEAD and
EMPTY distinguish latched instant-death and depleted states. Recoverable zero
remains GAUGE, not a failed state. Color and text both identify readiness/failure.

Draw using existing rectangle/text atoms with a fixed stack label; do not clone
gauge profiles, scan notes, advance replay/game state or allocate label strings.
Require nonnegative origins, positive width and at least 14 logical pixels of
height; validate dimensions/endpoints before mutation, clip the label to its component
and calculate bar width with wide integer arithmetic. Repeated drawing reads the
same snapshot without changing scores, gauges or replay frontiers. A preview
without an actual owner does not invent an active-play gauge.

Reserve an unused strip above solo counters and the right portion of each local
member's existing judge header for the gauge. Clip the neighboring judge label
to the remaining header width. Preserve playfield bounds, touch routing, saved/
peer/room comparison reservations and page layout. Local gauges remain member
specific across pages of up to four visible players and a roster of up to 64.
Browser HUD drawing stays on OffscreenCanvas in Worker; no Window DOM gauge or
new polling/render loop is permitted.

## Stepped failure fence and capture prefix

StepGameplay must fence its solo runtime after the first valid numeric gauge
failure, after independently consuming the actual committed report for gauge,
mine damage, score and replay capture. StepLocalGameplay fences only the failed
member. Numeric failure is game state, not a technical owner error; retain all
original report evidence and independent technical errors when both occur.
Expose gameplay_fence() for solo and gameplay_fence(player) for local observation.
Never discard the failure-causing operation, roll back judged stages, fabricate
release inputs, flush queued audio or mark a surviving cohort failed.

Subsequent acquisition still uses the portable fence's clock/sequence checks but
produces no new judgments or gameplay sound. Preserve the failed member's gauge,
score, song frontier and judge hash. Do not append post-fence acquisition or empty
advances to that member's capture. When capture accepts the failure operation,
the recording remains its exact committed failure prefix; a simultaneous capture
error retains the shorter accepted recording and its original error, without
claiming that it contains the missing operation. Other members continue normally,
including their own captures.
Shared local control progress must not regress when a frozen member reports its
earlier frontier. Do not confuse a member's frozen frontier with shared playback
or a surviving member's progress.

Reconstruct these new failure-prefix captures through the existing validated
replay pipeline and verify that hash, score, gauge and hazard observations match
the actual retained prefix. No new wire policy or retrospective rewriting of
older recordings is introduced here: legacy recordings continue to reconstruct
their own recorded operations. Configurable gauge/failure capture identity is
still separate work. This fixed default's only numeric failure is instant death;
custom pure profiles do not become unrecorded live options.

## Native game owner failure contract

NativeGameplaySession retains a borrowed default BmsGauge on the actual game
thread, independently of whether a UI publisher is attached. All three native
solo launchers supply a persistent gauge; each native local PlayerState owns its
own default gauge, including common preparation and all roster constructors.
Use the same common report policy and committed-frontier fence as stepped play.
Do not create OS-specific gauge or multiplayer rules.
Native pump admission rejects a nondefault profile before processing, even when
a typed caller supplies one through public state fields. Do not reset an existing
valid retained gauge when entering the pump. Configurable live profiles require
their own recorded policy identity before they can be admitted.

On each actual native report, independently consume gauge, capture, competition
and presentation observations even when one fails. Retain the original committed
report and independent failures on the technical error path. Fence numeric gauge
failure after consuming the failure-causing operation, before returning technical
errors. Numeric failure itself does not abort healthy cohort members. In local
play consume the whole actual report prefix, then fence only failed members;
an existing runtime poison stays poisoned.

Exclude post-fence operations from that player's capture while other members
continue. Keep a failed member's retained song/score/gauge frontier and the
shared queue intact. Successful failure-prefix captures must reconstruct the
same retained judge state through the existing replay pipeline. Capture rejection
retains its shorter accepted prefix and the original error. UI publication is
an observer; it does not own failure policy or supply invented clock evidence.

Compile-only Linux/workspace evidence does not establish Windows/macOS target
acceptance or real native audio/input/GPU behavior. Independent deferred native
owner fixtures use actual common report/pump paths and fake acquisition/output
evidence, covering headless and attached cases, prefix/error preservation,
replay reconstruction, and surviving independent local members.

Completion clear/fail decisions and
actual per-player audio stopping/output cleanup still need integration.
The [portable gameplay fence](REQ__gameplay-fence.md) is an explicit control
component for that integration. Its stepped and native connections above preserve
new capture prefixes; broader replay policy and per-player audio cleanup remain
separate integration work.
InstantDeath state alone does not establish playback termination. Keep the
high-level mine file admission guard until those owners are connected. Author
independent deferred fixed-point/configuration/atomicity and real solo/local/
replay prefix/failure fixtures; no application, browser, device or performance
acceptance is established by source compilation.
