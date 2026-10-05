# BMS mine source and runtime integration

## Original source and timing

Accept selected mine channels D1..D9/E1..E9 in a separate typed namespace,
mapping to original visible lanes 11..19/21..29. Mine payloads are direct
case-insensitive base36 damage values, independent of resource BASE16/36/62.
00 is a rest; 01..ZY retain exact half-percent damage units; ZZ is a distinct
instant-death value. Never interpret a damage token as a WAV reference.
Optional WAV00 remains an opaque explosion-sound definition; absence requires
no fabricated asset or implicit sound. Damage semantics follow the public
[Angolmois developer documentation](https://github.com/lifthrasiir/angolmois/blob/master/INTERNALS.md#data-commands);
BeatKernel preserves the full nonfatal range instead of discarding values
above 100%. This is an explicit policy, not full historical compatibility.

`BmsChart` stores original mine events and an independent exact tick grid
encompassing gameplay resolution. Record beat, original lane, typed damage,
independent ordinal and physical source line. Mine subdivisions must never
enlarge existing gameplay, BGM, BGA or invisible grids or change normal object
identities. Merge same-lane positions under existing Reject/LastWins rules in
the mine namespace only. Preserve overlapping visible/hold/invisible source
positions for the runtime policy; do not silently convert or discard mines.
Seeded conditional selection, source/line/grid bounds and combined raw/final
source counts include mines, also before LastWins replacement.

`compile_mines()` returns a separate sorted `ScheduledMine` timeline using the
existing checked core timing compiler and pre-STOP same-beat semantics.
Validate fabricated grids, lane identities, duplicate positions/ordinals,
rescaled BPM/STOP arithmetic and combined source limits. Mine compilation does
not insert judged objects or automatic BGM into the returned ordinary chart.
Empty mine timelines preserve existing compilation and replay setup behavior.
Author parser, timing, identity, duplicates, BASE, seeded, cap, malformed typed
source and long-duration fixtures for deferred execution.

## Integration boundary

### Common optional sound primitive

The [core hazard-sound contract](REQ__hazard-sounds.md) publishes sounds from
actual Triggered report prefixes at the normalized output frontier, after
normal and invisible press sounds. Its immutable exact-ID bindings interpret no
BMS damage values and choose no fatal-sound policy. BMS still must prepare the
optional WAV00 bindings/assets and install them in all actual owners with local
voice isolation, source-aware identity and output/completion handling before
mine source admission. A core sound primitive alone does not establish WAV00
playback or complete mine support.

### Shared retained mine rendering

PlayerChart retains an immutable, separately typed PlayerMine timeline containing
original full-width ordinal, ordered lane index, compiled timestamp and typed
damage. Mines remain outside normal PlayerNote/object/progress/score identity.
Use binary searches over the prepared timestamp order and reusable index scratch
to query an inclusive visible window in wide integer arithmetic. Negative
windows clear scratch; more than 2048 visible mines fail explicitly and clear
scratch rather than silently dropping markers. Empty mine sources allocate no
mine query backing storage. Query work depends on visible markers, not all
historical mines.

Scene and PlayfieldCache combine visible mines with existing note instances in
the same retained GPU packet and ordered playfield batch. Nonfatal mines are red
chips; instant-death mines are magenta chips, distinct from green normal heads.
Reuse the existing head primitive and local-epoch drift/clip shader, without
turning mines into scored notes or adding Window rendering. Each mine needs one
instance; retain the normal-note 2048 and mine 2048 visibility budgets and size
the common buffer for three normal primitives plus one mine primitive per cap.
Prepared metadata, visible-set, bounds, lookahead or backward-time changes
invalidate the cache; a stable visible set within its local epoch reuses the
existing instance Arc. Validate both queries and lane references before changing
the scene's playfield batch/cache. The actual solo and paged-local native/browser
composers use this common Scene path; browser drawing stays in Worker.

This draws the original timeline, without inventing hit/avoid flashes, judge
advances, fatal stop policy or gauge outcomes. Author independent deferred
window-oracle/cap/scratch, retained geometry/cache, real composer/local-page and
long-time fixtures. Device/GPU/browser execution and measured performance remain
unverified; complete source admission remains guarded.

### Mine lanes, practice eligibility and full-song completion

PlayerChart prepares the sorted union of visible, invisible and mine lanes,
including mine-only channels, and includes the latest compiled mine time in
its presentation duration. Do not manufacture normal PlayerNote objects or
score totals for mines. Preserve existing ordering, ordinary note identities
and empty-source behavior. Validate actual mine timing before publishing a
presentation; default touch geometry and coverage use the resulting lane union.

Practice preparation may retain a mine-only suffix when at least one original
compiled marker is at or after the selected start. Earlier markers retain their
original timing/identity for fresh reconstruction; they do not by themselves
make an otherwise empty suffix playable. Existing non-mine eligibility remains.

SongCompletion includes each mine's effective judge boundary, translated back
to song time using the actual input offset, with checked arithmetic and a
strictly later one-nanosecond deadline. Mine boundaries have no normal-note late
window. Calibration extent includes that deadline. Completion also requires
actual consumption of every configured hazard, rather than inferring completion
from presentation time or an empty ordinary chart. Constant-time read-only
JudgeEngine hazard-count and remaining-count accessors expose the existing
cursor without changing canonical bytes or consumption. A mismatching supplied
judge refuses before adopting completion/output evidence. Existing mixer and
native presentation drain evidence remains required. Check actual remaining
hazards on every observation, including after judge snapshot restoration; newly
pending hazards revoke the earlier output-drain barrier before completion.
This does not choose
fatal stop/gauge policy, add WAV00 or enable source admission.

### Shared committed damage summary

Native presentation stores `mine_damage: MineDamageSummary` per actual
LocalPlayerSnapshot. The solo compatibility snapshot mirrors its sole member;
multiple members expose no fabricated aggregate damage. Actual solo/local
report publication consumes each report's hazard events through the same
summary. Preflight score, damage and pressed ownership for the entire local
batch before mutating any member or lifecycle. Invalid damage, overflow,
unknown/duplicate members or invalid input leave prior publication unchanged.
Empty reports preserve damage with no additional heap allocation or history
clone for damage. Cancellation, pause and final cleanup retain admitted damage.

Native replay publication accepts the actual reconstructed cumulative summary
alongside incremental normal results and pressed mask. Assign that summary;
do not add it a second time. Repeated prefixes are idempotent. Reject regressing
counts/damage, unlatching death and impossible numeric summaries before any
score/history/pressed/lifecycle mutation. Legacy replay publication lacking
damage metadata preserves existing damage. Both actual replay presentation
and pause-boundary calls supply ReplayVisual's summary. Snapshot struct literals
must initialize the new field. This exposes retained evidence for future HUD
and gauge use without claiming it is drawn or changes play outcome.

`MineDamageSummary` consumes actual one-shot HazardEvents and retains checked
full-width triggered/avoided counters, exact accumulated nonfatal half-percent
damage units and a latched instant_death flag. Validate every opaque value as
1..1295 before committing a batch. Avoided markers add no damage; triggered ZZ
latches instant death but is not converted into numeric percent damage. Values
above 100% remain exact, as the source policy already requires. Invalid values
or counter overflow leave the entire prior summary unchanged. Consumption adds
no per-frame scan, collection allocation, asset access or ordinary JudgeEvent.
Callers must pass each committed result once; this summary does not deduplicate
or re-judge arbitrary injected events.

Stepped solo consumes each actual RuntimeReport, including committed prefixes
on later judge/audio failures. Local members have independent summaries and
consume their actual reports in member order. Expose read-only summary accessors;
rejected pre-report acquisition and unknown members cannot change summaries.
Summary failure fences the owner while retaining the actual report and other
score/capture errors. The public local-member failure record adds a mine_error
field; external literal constructors must initialize it.

ReplayVisual consumes each actual recorded judge operation's hazard report
immediately after success. A display target may apply several operations whose
later empty reports must not erase earlier damage. Equal presentation targets
and targets before the next recorded operation add nothing; never synthesize an
extra advance from display time. Regressions reject before summary mutation.
StepReplay borrows this same accumulated summary, rather than applying a second
replay-specific damage rule. Fresh playback/practice owners start at empty state;
restored judge hazards do not imply restored application counters by themselves.

This layer records exact damage and fatal evidence, preparing actual gauge/death
integration without choosing a normal-note gauge curve, initial gauge or fail
threshold. It does not stop playback, enqueue WAV00, render mines or grant source
admission. Native presentation/report consumers and complete gauge/death/audio/
completion behavior remain required work. Author independent deferred atomicity,
raw-range, live/local/replay equality and failure-prefix fixtures.

### Shared plan and judge construction

`MinePlan::prepare(source, max_markers)` validates the source mine count before
timing preparation and compiles through `BmsChart::compile_mines`. Retain exact
ScheduledMine time/lane/damage/ordinal/line for application presentation. Map
ordinal to full-width HazardId, original lane to its logical control and raw
damage to the opaque u64 hazard value without applying gauge or allocating audio.
The core timeline retains compiled time/ordinal order. Empty plans expose no
configured timeline, preserving the exact legacy judge configuration and hash.

One `prepare_judge` composition accepts the caller's matching compiled ordinary
chart, source, profile, input mode and marker budget. Prepare hazards before
constructing/installing a new judge; enable contact ownership for contact mode
with either invisible sounds or mines, including a mine-only empty scored chart.
Stepped solo/local, shared local-member preparation, source-aware replay setup
and offline construction use this same helper. Local-member binding coverage
includes visible, invisible and mine lanes; a missing mine-only control refuses
setup rather than silently dropping input. A failing preparation returns no
judge. Replay setup identity uses the actual configured pristine judge, so hazard
time/control/value/ID changes affect compatibility without a separate OS policy.
Practice selection retains original absolute source times; it never rebases mines
or invents prior occupancy. Earlier markers processed by a fresh owner therefore
observe its fresh empty state. Full practice presentation/damage policy remains
an integration requirement below.

The caller must provide the ordinary chart compiled from the supplied selected
source. This helper does not verify PCM, apply gauge/death or publish outcomes.
NativeJudgeConfig exposes source-aware pristine judge construction using this
same helper, existing asymmetric windows, signed offset and ButtonOnly policy.
Linux ALSA, Windows WASAPI/optional-ASIO and macOS CoreAudio solo setup call it
before competition identity, audio start and optional recording, retaining the
existing cleanup boundary. Platform code does not interpret damage or rebuild
hazard ordering. Recording and competition use the actual configured judge's
initial hash. Disabled capture still ignores unused capture settings.

Consumer-side BMS integration remains pending;
the shared playable-source admission guard stays in force. Author independent
plan/type/budget/timing/contact/snapshot/replay identity and actual-owner fixtures
for later execution, with no performance or platform acceptance claim.

The [shared judge hazard contract](REQ__judge-hazards.md) defines one-shot
occupancy processing, simultaneous operation order and complete checkpoints.
Its engine component is the next integration layer; it does not by itself
enable BMS gameplay admission or apply BMS gauge and sound policy.

During source integration, shared `prepare_from_source` rejects any nonempty
mine timeline immediately after actual parsing and before gain/replay/asset
lookup, reads, decode or PCM allocation. Empty/rest-only/inactive mine rows
retain the ordinary loading path. This prevents loading a playable chart which
silently omits its hazards. Adapter parsing and explicit timing are available;
the temporary admission refusal is removed only when actual portable hazard
processing is installed in live, local, replay, practice and offline owners.

Full mine support still requires contact/held-button occupancy at original-song
mine times, deterministic simultaneous release/press ordering and per-mine
one-shot outcomes shared by live and replay. Rendering, gauge damage/instant
death, WAV00 output, local voice separation, completion and source-aware replay
identity must use those actual outcomes. Do not claim these integration
requirements complete from a separate source timeline or compile-only checks.
Execution, platform/audio/browser acceptance and required review/QA remain
deferred under the active task; the whole player Goal remains open.
