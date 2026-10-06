# Common BMS gauge observations

Gauge state belongs to the BMS runtime and consumes actual committed judge and
hazard reports. It does not reinterpret opaque grade numbers, judge inputs,
emit sounds, rewrite scores or infer new input. Use one shared BmsGauge policy
for stepped solo/local playback and incremental replay presentation.

## Fixed-point policy

One percentage point is 1,000,000 integer units; maximum level is 100,000,000.
GaugeProfile specifies initial/clear levels, signed default hit and miss deltas,
an optional fail-on-empty latch, up to 64 explicit opaque grade overrides and
resolved level-dependent dynamics described below.
Validate initial/clear bounds, override capacity and duplicate grades at setup.
Sort overrides once; hot observation uses binary lookup and no allocation.
Signed deltas use wide arithmetic and clamp to [minimum_alive, maximum],
including i64 extremes; the default minimum is zero. Depletion and instant death
may latch a failed level of zero below that living minimum. An unlisted hit uses
the explicit fallback delta, not grade ordering.
Every actual normal judged stage contributes once, including hold head/tail;
invisible presses and mine markers contribute no normal-note stage recovery.

The documented BeatKernel default is initial 20%, clear threshold 80%, +1% per
successful judged stage, -6% per missed stage, and recoverable zero. It is an
explicit application policy, not LR2/IIDX/Angolmois gauge compatibility. The
builtin policy does not consume TOTAL. Selected native BMS gauges below consume
retained original TOTAL/stage counts without changing the omitted-option default. Pure custom profiles support
other grade deltas and fail-on-empty policy. Native launchers, browser launch
defaults and replay defaults retain the fixed documented policy. Stepped owners
can accept resolved profiles in pristine setup and retain them in result archives
and standalone capture/replay setup identity as described below.
Native simple gauge selection is described below; browser and richer policy selection remain unfinished.

Observe the report's normal results first, then its hazard events in original
order. Validate every hazard value in 1..1295, including avoided outcomes and
events after failure, before committing a batch. Nonfatal triggered values cost
exactly value * 500,000 units before the profile's living floor/depletion rules;
with default dynamics, damage exceeding the whole level clamps to zero.
Avoided events cost nothing. Triggered 1295 (ZZ) sets level zero and latches
InstantDeath independently of fail-on-empty. These mine meanings follow the
[original Angolmois documentation](https://github.com/lifthrasiir/angolmois/blob/master/INTERNALS.md#data-commands);
the full nonfatal range remains BeatKernel's explicit policy. Empty failure
latches Depleted; retain the first failure reason and freeze subsequent numeric
changes. An initial zero profile with fail-on-empty is initially Depleted.
Invalid report batches leave the previous gauge snapshot exactly unchanged.
Clear readiness is a level/failure predicate, not evidence that a song finished.

## Adapter #TOTAL and LR2 gauge rules

beatkernel-bms owns pure, allocation-free, float-free LR2 gauge rules in the
same 1,000,000-units-per-percent fixed point. They classify six BMS judgments
(PGREAT, GREAT, GOOD, BAD, POOR, empty POOR); mapping opaque core grades and
miss/empty-press reports onto them stays with the application. Judged stage
count N is one per gameplay object plus one per hold tail, matching the core
judge's head/tail stages; mines, BGM and invisible keys never count. This
deliberately counts core head/tail stages rather than one long-note object.
Groove-family recovery uses TOTAL divided by this stage count; each delta
truncates independently and gauge-level clamping may discard recovery. A full
PGREAT run therefore need not add exactly TOTAL percent to the retained level.

#TOTAL is a plain positive decimal (optional `+`, at most 18 digits) read with
millionth precision; finer digits truncate. Zero, negative, exponent, malformed
or u64-overflowing values are Invalid, absence is Absent, and both use the LR2
default `160 + (N + clamp(N - 400, 0, 200)) * 0.16` exactly, saturating. Each
resolution reports Declared/Absent/Invalid provenance; nothing is silently
substituted. A chart with N = 0 has no defined gauge and is rejected at setup.
The default formula is documented by the
[LR2oraja project](https://github.com/wcko87/lr2oraja#6-revised-total-calculation-for-charts-without-a-specified-total-value).

Per-judgment base percentages follow the LR2 table published by
[LR2oraja gauge properties](https://github.com/wcko87/lr2oraja/blob/lr2oraja/src/bms/player/beatoraja/play/GaugeProperty.java),
reimplemented here as data:

| Variant | PG | GR | GD | BD | PR | Empty PR | Init | Floor | Fail below | Clear |
|---|---|---|---|---|---|---|---|---|---|---|
| AssistEasy | 1.2T | 1.2T | 0.6T | -3.2 | -4.8 | -1.6 | 20 | 2 | — | 60 |
| Easy | 1.2T | 1.2T | 0.6T | -3.2 | -4.8 | -1.6 | 20 | 2 | — | 80 |
| Groove | 1.0T | 1.0T | 0.5T | -4 | -6 | -2 | 20 | 2 | — | 80 |
| Hard | 0.1 | 0.1 | 0.05 | -6D | -10D | -2D | 100 | 0 | 2 | alive |
| ExHard | 0.1 | 0.1 | 0.05 | -12D | -20D | -2D | 100 | 0 | 2 | alive |
| Hazard | 0.15 | 0.06 | 0 | -100 | -100 | -10 | 100 | 0 | 2 | alive |

T is TOTAL/N. D is max(10 / clamp(floor(TOTAL/16) - 5, 1, 10), note factor),
where the note factor is 10 for N <= 20, 8 + (30-N)/5 below 30, 5 + (60-N)/15
below 60, 4 + (125-N)/65 below 125, 3 + (250-N)/125 below 250, 2 + (500-N)/250
below 500, 1 + (1000-N)/500 below 1000, and 1 otherwise; the LR2oraja step at
N = 30 is retained. Products are exact rationals truncated toward zero once at
setup; recovery saturates at a full gauge. The damage and state reference is
[LR2oraja GrooveGauge](https://github.com/wcko87/lr2oraja/blob/lr2oraja/src/bms/player/beatoraja/play/GrooveGauge.java).
Hard damage is multiplied by 3/5,
truncated, while the level is strictly below 32%. Levels clamp to [floor,
100%]; a level strictly below the fail threshold becomes zero, which latches
failure and freezes all later judgments. Groove-family gauges never fail and
qualify only when the final level reaches their border. Survival gauges qualify
while alive. Qualification is not evidence that a song finished.

BmsGaugeState is Copy, has no heap or dynamic dispatch, and applies one
judgment with constant work. Player launch defaults still use the documented
default profile. Stepped setup may consume resolved adapter rules through
`GaugeProfile::from_bms_rules` and `configure_gauge`; native gauge selection is
described below. Historical mine compatibility, course gauges, richer judgment
presets and browser policy selection remain separate integration work. Recorded gauge setup is specified below.

## Actual owners and remaining terminal control

StepGameplay and each StepLocalGameplay member initially own a default BmsGauge,
optionally replaced by a resolved profile during pristine setup, and consume
actual committed report prefixes even when judge/audio/score/capture
postprocessing also fails. Retain the original report and independent errors
when gauge aggregation fails, fence technical processing and expose the previous
atomic gauge state. Numeric gauge failure is readable game state; it is not a
technical error and does not discard committed audio or falsely mark output as
drained. Do not fold independent local gauges into one cohort gauge.

ReplayVisual observes normal and hazard results immediately after each actual
recorded operation. Repeated/equal display targets, time beyond a recorded
prefix and unrecorded display advancement produce no extra gauge changes.
StepReplay delegates to the same gauge rather than observing results twice.
Legacy/default replay setup bytes and core judge identity stay unchanged.
Policy-aware consumers use the recorded gauge wrapper described below.

## Native publication contract

Each local presentation member retains its own default BmsGauge. Native solo
and group report publication updates gauge, score, mine summary, pressed state
and history atomically after whole-batch validation. An invalid later member
must leave every earlier member unchanged. Empty deadline reports do not clone
gauge profiles or allocate gauge scratch. The legacy solo gauge mirrors exactly
one member; a multi-member cohort has no aggregate gauge.

Actual replay presentation copies the authoritative ReplayVisual gauge, including
pause-boundary publication. It must not rebuild gauge order from cumulative mine
damage or apply incremental judgments a second time. Legacy chart registration
admits only the documented default profile. Native replay chart registration may
admit a nondefault profile from a pristine validated ReplayVisual, only when its
exact compiled chart matches and no chart/roster/prefix is already registered.
Store that expected policy for the session; every later replay gauge must match.
Refusal preserves chart, roster, scores, pressed state and gauge atomically.
An unchanged absolute replay gauge remains unchanged on repeated publication.
Legacy replay APIs lacking an authoritative gauge cannot establish mine-aware
gauge accuracy: they observe actual normal judgments without synthesizing mine
events from a cumulative summary. Actual replay callers must use the full gauge
publication API. Reject a default replay gauge whose instant-death state disagrees
with its summary. For survival profiles, depletion may precede a later mine death
in a retained full log, so cumulative mine death may coexist with first-latched
Depleted. Depleted requires a fail-on-empty policy; every failed gauge has zero
level. Reject changed policy or any change to an already frozen failed state
before presentation mutation. Ordinary level loss/recovery is not a monotonic counter.

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
the actual retained prefix. Legacy recordings continue to reconstruct their own
recorded operations without retrospective rewriting. Stepped configurable
gauge/failure setup identity is specified below; the core replay envelope remains
unchanged. The fixed default's only numeric failure is instant death. Resolved
profiles are retained by result archives and standalone capture/replay setup.
Native launchers use the documented default when --gauge is omitted; browser
selection remains separate work.

## Native game owner failure contract

NativeGameplaySession retains a borrowed BmsGauge on the actual game
thread, independently of whether a UI publisher is attached. All three native
solo launchers supply a persistent gauge; each native local PlayerState owns its
own default gauge, including common preparation and all roster constructors.
Use the same common report policy and committed-frontier fence as stepped play.
Do not create OS-specific gauge or multiplayer rules.
Native nondefault admission below validates caller-supplied profiles before
processing. Legacy default entry does not reset retained gauges; nondefault
entry requires the initial gauge and pristine usable owner. Enabled recording
and competition must retain and validate the chosen policy identity.

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

## Failed-player pressed feedback

After a valid numeric gauge failure, clear only that player's retained display
button/contact owners and pressed lane mask. Native publication and browser
Worker solo/local observation perform this before publishing or drawing the
failure report, including simultaneous technical failures. Subsequent reports
must not reacquire display ownership for a failed gauge. Recoverable zero and
clear readiness alone do not clear a player's keys. Healthy cohort members keep
their own ownership, including exact full-width device/contact identities.

Replay presentation clears its display ownership after the actual recorded
operation first produces gauge failure. Later legacy recorded operations still
reconstruct their original judge state but cannot reacquire failed-player
display ownership. Authoritative replay publication also clears the retained
mask regardless of a supplied stale absolute pressed mask. Equal display targets
remain idempotent. Do not synthesize release events, mutate judge held state,
rewrite captures or clear shared audio as part of this presentation policy.
Whole-batch publication validation still precedes mutation; invalid reports leave
all members unchanged. Add no Window rendering or gameplay work.

The [completed live result contract](REQ__bms-completed-play-result.md) separates
actual output completion from scoped clear/fail classification. Native/UI/archive
result publication and recorded-prefix result identity still require integration.
The [scheduled gameplay voice stop](REQ__gameplay-sound-stop.md) defines the
shared audio component and stepped/native owner connections. Explicit stepped ACK
and native producer evidence connect scheduled Stops to their output completion
paths; source/compile evidence does not establish physical audio acceptance.
The [portable gameplay fence](REQ__gameplay-fence.md) is an explicit control
component for that integration. Its stepped and native connections above preserve
new capture prefixes; broader replay policy and per-player audio cleanup remain
separate integration work.
InstantDeath state alone does not establish playback termination. Keep the
high-level mine file admission guard until those owners are connected. Author
independent deferred fixed-point/configuration/atomicity and real solo/local/
replay prefix/failure fixtures; no application, browser, device or performance
acceptance is established by source compilation.

## Resolved dynamic policy and record identity

Application profiles may carry a living minimum, a strict failure-below level
and a strict low-level judgment damage reduction (3/5). Zero dynamics retain the
legacy BeatKernel policy. Survival depletion latches at zero; grade classes are
explicitly mapped by callers, never inferred from opaque grade numbers. Mines
retain raw existing application damage and instant death, without judgment guts.
This does not assert full LR2 mine/empty-judgment compatibility.

`StepGameplay::configure_gauge` and `StepLocalGameplay::configure_gauge` accept
resolved profiles only while input setup remains pristine, before activation,
processing or capture configuration. A local member keeps its own profile, but
capture configuration for any member closes gauge setup for the whole cohort.
Refusal preserves the existing profile and owner usability. Native-save and
stepped completion archives must copy the entire resolved profile; fixtures
exercise solo and mixed-policy local completion, including original player IDs.

Records with nonzero dynamics use versions 4/5, preserving all resolved fields
(and optional comparison snapshots). Legacy/default records keep versions 1/2/3
and old decoding supplies zero dynamics. Copied profiles and decoded live-level
invariants must preserve these fields. Native default behavior and old playback
must remain unchanged; browser selection and richer judgment presets remain
unfinished.

## Recorded gauge setup

Nondefault stepped policies must be included in capture and competition setup
identity. A versioned `bms-gauge-setup/v1:` options wrapper carries the length of
the existing judge options followed by the complete fixed-point gauge profile,
sorted grade overrides and dynamics. Default policies retain exact legacy
options bytes; a wrapped default policy or nested wrapper is noncanonical and
must be refused. Validate exact extents, grade capacity/order, boolean tag and
profile invariants before allocation or playback. Header budgets include the
wrapper; a refused capture setup must not alter the owner or existing capture.

Section-aware replay validation must regenerate the entire setup identity with
the recorded policy. Tuple APIs that cannot retain gauge policy refuse wrapped
setups. Incremental visual/stepped replay and audio sound-stop planning use the
recorded profile, including depletion from ordinary judgments without mines.
Record catalog comparisons must include gauge policy. The kernel replay format
and judge hash remain unchanged: the application options own this policy.
Native simple gauge selection is described below. Browser selection, richer
judgment presets and complete LR2 compatibility remain unfinished.

The native output-only replay command uses policy-aware validation before PCM
asset reads, pristine replay chart registration, and recorded-policy audio/visual
planning for unlimited and finite recordings. The finite plan retains the exact
upward-rounded playback endpoint, including preroll; Mixer and ReplayPause use
that same immutable endpoint. Display targets clamp at the original-song end.
Completion requires a genuine endpoint render marker, exactly all feeder-admitted
commands consumed/applied, and native presentation crossing the marker's physical
frame. Manual pauses may shift that physical frame; never assume it equals the
configured playback frame. Validate connected producer, render/counter chronology,
stable endpoint and frozen post-end state before publishing visual progress.
Terminal reports are paused: retire feeder credits from their validated playback
cursor without admitting more commands. Check exact endpoint execution before
visual publication even if those credits have not yet retired. An irreversible
endpoint completes when its presentation and recorded-prefix proof are present,
including a pending manual pause; it must not wait for an impossible resume.
Missing render/presentation evidence cannot complete playback. Ordinary idle
drain is insufficient for a finite cutoff, and wall limits remain truncation.
Headless publication validates gauge consistency and remains a presentation
no-op. This does not establish physical audio/GPU or driver acceptance.

## Common play policy preparation

An immutable resolved play policy owns both JudgeProfile and GaugeProfile.
Builtin selection preserves the existing single grade-one early/late window,
signed calibration offset and BeatKernel gauge; native preparation uses this
same common builder. Selection names are exact `beatkernel`, `assist-easy`,
`easy`, `groove`, `hard`, `ex-hard`, `hazard`; other spellings are refused.

For the six adapter gauge kinds, callers provide explicit opaque JudgeGrade,
BMS hit class (PGREAT/GREAT/GOOD/BAD), and nanosecond early/late windows. Never
infer classes from grade numbers or invent an historical timing table. Reject
empty or more than 64 windows, duplicate grades, invalid JudgeProfile windows,
and POOR/empty POOR as hit classes. Resolve TOTAL and judged-stage count from
the supplied original source; retain TOTAL fallback diagnostics. Unknown-hit
fallback is explicitly PGREAT, while every prepared hit grade has its own
mapping and core misses use POOR. Empty presses remain separate integration.

Preparation is control-side; owning judge/gauge parts needs no dynamic dispatch
or extra runtime wrapper. Replay capture retains the resulting windows/deltas
and dynamics through the existing full setup identity. Native simple selection
uses this builder as described below. Browser/per-member/live selection,
nondefault competition, class-aware score labels and complete mine/empty
compatibility remain unfinished.

## Native policy preparation ports

NativeGameplayHost has a cold policy preparation port accepting exact ordered
player IDs and borrowed GaugeProfiles. Unsupported hosts reject nondefault
policies; the explicit headless host supports them without publication, and the
score observer forwards preparation to its wrapped host. The player adapter
initializes each member's gauge atomically once, after exact chart/roster
registration and before any report, replay policy or completion. IDs/order must
match the entire original roster (1..64), preserving full-width IDs; failures
leave every member, score, timeline and publication unchanged. No clocks or
gameplay events are synthesized. Empty deadline reports after preparation
continue using the same independent member profiles.

Native policy capture preparation requires an unprocessed judge whose profile
matches the immutable resolved policy. Disabled capture still checks that policy
boundary but performs no source identity acquisition. Enabled capture uses the
existing ButtonOnly native setup and records the complete gauge through its
canonical wrapper; limits/refusal preserve the judge and policy. Legacy helper
and default host behavior remain unchanged. These preparation APIs are invoked
by the guarded common pumps and native launchers described below. Custom
competition identity, richer/per-member/live/browser selectors and class-aware
scores remain separate integration work.

## Native nondefault admission

Builtin pump setup retains its existing behavior. A nondefault native member
requires an unprocessed judge and the exact initial gauge snapshot before any
pump control/device effects. Poisoned or already fenced runtimes are refused even
when the judge has not processed an operation. If capture is enabled, its records must be empty;
the canonical header must match the gauge, judge windows, original pristine judge
hash, normalized host domain and original-song start/end. Derive start from the
configured song origin plus playback/stream-origin preroll, with checked wide
arithmetic. Native ButtonOnly rules and exact chart-identity extents are required.

Competition ports may supply their immutable expected header or explicitly
declare themselves policy-agnostic (disabled Noop observers). Nondefault members
reject opaque competition; supplied identities must match the same setup and,
when capture exists, its entire header. Shared cohort networking remains refused
for nondefault policies until it supplies policy-aware member identities.

For a cohort containing any nondefault member, the usable RuntimeGroup's original
roster order must exactly equal session.states. Every member, including builtin
members, must have a pristine judge, initial gauge, fresh default score,
last_song equal to configured song_origin and no gameplay fence. Validate every
capture/competition identity before preparing the host's entire ordered roster.
Nondefault admission permits at most 64 judge windows. Reserve cold scratch and
policy-row storage before host preparation. Only a successful setup invokes the cold preparation port, before pump clock,
device observation or input acquisition. Rejection preserves judge/gauge/capture
state. Native selection below uses these common pumps. This does not establish
new weighted/class-aware scoring or physical timing.

## Native gauge selection

Native CLI and common retained settings expose `--gauge` with the exact seven
selection names; omitted/empty draft values preserve `beatkernel`. Explicit CLI
empty, unknown, non-lowercase, missing or duplicate --gauge values are refused
by all three native parsers before play. Selected BMS
gauges use the existing configurable early/late window as a single PGREAT hit
class, with POOR for misses and the same signed offset. This is explicit simple
timing, not a historical LR2 judgment-window preset or full scoring compatibility.

Practice preparation retains original TOTAL provenance and full judged-stage
count before excluding earlier heads. Resolve the selected policy from that
context once, then use the same windows/gauge in solo or every local member,
capture and host admission. Original context uses no additional asset read.
Builtin behavior and recordings stay byte-compatible. Nondefault selection with
ghost or network competition is explicitly rejected while competition preparation
still uses legacy identities; do not silently disable it or start its resources.
Current-draft record comparison resolves the same policy from original source.
Live gauge changes, per-member selectors, graded timing presets/class-aware
scores, custom competition and browser selection remain separate work.
