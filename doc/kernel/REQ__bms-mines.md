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

The [common gauge contract](REQ__bms-gauge.md) defines fixed-point recovery,
mine damage and latched failure state from committed reports. Actual terminal
playback control, native/HUD publication and final file admission remain required;
a readable InstantDeath snapshot alone does not complete mine support.

### BMS WAV00 plan and stepped live installation

MineSoundPlan prepares validated original mine IDs with optional SampleId(0)
only when WAV00 is defined and at least one nonfatal mine exists. Fatal ZZ has
no explosion binding; this sound policy follows the documented
[Angolmois instant-death behavior](https://github.com/lifthrasiir/angolmois/blob/master/INTERNALS.md#data-commands).
Absent WAV00, fatal-only and empty sources create no implicit sample/voice.
Validate mine capacity/timing even when sound is absent. Audible bindings use
the source WAV gain and one reusable voice per logical mine lane, ordered by
control and allocated above supplied gameplay, BGM and invisible press voices.
Checked exhaustion refuses the whole plan; no PCM load or command is fabricated.

RuntimeGroup installs exact roster-ordered hazard timelines atomically before
start, refusing cross-member aliases and collisions with gameplay/reserved BGM
or already installed press voices. Both press and hazard installations reserve
their admitted voices so the opposite installation order also rejects collision.
VoiceAllocator remaps hazard bindings with within-lane aliases retained and
atomic exhaustion. Local preparation validates optional WAV00 bank membership,
then reserves distinct member voices after every existing actual voice.

StepGameplay's actual solo and local setup installs the plan after press sounds,
including StepLocalGameplay's delegated setup. Missing audible WAV00 PCM fails
before activation. SongCompletion includes optional explosion PCM tails in its
prepared calibration extent, without replacing real mixer/output drain evidence.
Extend the existing source sound identity with a versioned mine-sound fingerprint
only for audible WAV00 plans; hash semantic IDs/control/time/gain/sample selection,
not resource paths or remapped output voices. No audible mine extension preserves
the previous identity exactly. Mine fingerprint wire order is the domain
`beatkernel-bms/mine-sounds/v1`, little-endian u64 binding count, f32 gain bits,
SampleId(0), then ascending-ordinal rows of u64 ordinal, u32 control and i64 song
nanoseconds. Nonfatal damage magnitude is excluded here because pristine judge
identity already includes it. Compose the existing optional invisible fingerprint
and mine fingerprint with `beatkernel-bms/input-sounds/v2`, a one-byte invisible
presence flag, its u64 fingerprint when present, and the u64 mine fingerprint.
Both domains use the existing semantic FNV-1a64 convention, not a cryptographic
asset digest. Existing source-aware capture/replay validation
consume that common identity. Native, replay, offline installation and internal
WAV00 asset preparation are specified below; high-level admission remains gated.

Author independent deferred plan/voice/identity, atomic group-order collision,
real solo/local report-to-command and completion-tail fixtures. Gauge/fatal-stop
policy and guarded asset admission remain
unfinished. Execution and acceptance remain deferred.

### Native WAV00 preparation and activation

Native solo preparation uses the common MineSoundPlan with already prepared
press voices and the actual PCM bank, before bank movement or output startup.
No-mine sources retain the unconfigured path. Audible missing WAV00 PCM fails
preparation; absent WAV00 and fatal-only sources require no explosion PCM.
Linux, Windows and macOS solo owners install that timeline after press sounds
and before finite-end activation, without platform-specific sound semantics.

The common native cohort preparation includes mine lanes in binding coverage
and prepares member-disjoint hazard timelines after gameplay/BGM/press voices.
It validates all sound preparation before opponent loading. PreparedCohort
carries those timelines to a common activation entry point, which installs
press and hazard timelines before setting the song end. Existing legacy and
press-only activation APIs remain available as empty-hazard wrappers. All three
actual platform local owners pass the prepared hazard timelines through that
same entry point. No sound configuration publishes commands during activation.

Author independent deferred actual native-plan/Runtime/Mixer and cohort fixtures
for routing, held triggering versus avoidance/fatal silence, simultaneous normal
and press sounds, disjoint member voices, finite-end fences, atomic refusal and
missing PCM. Native source changes and portable composition compilation do not
prove Windows/macOS device behavior. Replay/offline scheduling is specified
below. Internal WAV00 asset preparation is shared under guarded admission.
File mine admission and complete gauge/fatal-stop policy remain unfinished.

### Replay and offline WAV00 consumers

Replay audio validates the same source sound identity and prepares the common
MineSoundPlan against actual PCM and already reserved press voices. Missing
audible WAV00 PCM fails planning. When mine sounds are audible, replay the
original recorded operations through a pristine source-aware judge once. Consume
each operation's normal hits, fresh press selection and hazard report immediately
in that order, including hazards emitted by Advance. Map hazard commands from
the recorded operation's song time to the output origin, removing section start
once and adding preroll once. Do not substitute the original mine timestamp for
a delayed recorded operation, reinterpret fatal damage, seek the engine from
scratch, advance past the recorded prefix or reuse an old operation's report.
Preserve original operation order for equal output times. BGM retains its
existing background-before-gameplay ordering. Finite output rejects commands
whose rounded execution frame reaches the endpoint. No audible mine extension
retains the existing replay sound scheduling path and final reconstructed hash.

Offline rendering installs common hazard sounds after press sounds, against
actual PCM. Its chronological schedule includes explicit judge advances at
compiled mine boundaries, after BGM and synthetic normal-note inputs at the
same song time. This resolves held mine triggers before rendering their output
frames instead of delaying them to the next ordinary input or final report.
Mine-only lanes may appear in the binding map but generate no artificial presses;
only actual normal notes supply synthetic input. Fatal and absent-WAV00 mines
still follow actual judge outcomes without explosion commands. Keep the existing
frame extent, output clipping, queue failure evidence and no-mine schedule.

Practice section preparation already retains the original PCM bank, including
SampleId(0), and allocates BGM suffix IDs strictly above zero. Preserve the WAV00
PCM unchanged through practice/replay preparation; do not crop it like a crossing
BGM tail or relabel it as a suffix. Add independent deferred fixtures using
actual capture/reconstruction, Runtime, offline rendering and Mixer paths for
equal-time order, held contacts, Advance-triggered hazards, replay prefixes,
section/offset/preroll/finite mapping, missing PCM and silent/fatal sources,
block-partition consistency and unchanged WAV00 across section BGM slicing.
Internal asset preparation is specified below. File mine admission and complete
gauge/fatal-stop remain pending.

### Common WAV00 asset preparation under guarded admission

The shared audio asset selection/loader is reused by prepare_from_source for
ordinary visible, BGM and invisible resources. Extend its referenced sample
union with SampleId(0) only for a defined WAV00 and at least one compiled
nonfatal mine. Validate original mine timing/capacity even for silent sources;
fatal-only, absent-WAV00, empty/rest and inactive mine sources invent no asset.
Apply the existing unique-ID PCM sample limit to the full union before resource
acquisition, retaining ascending sample-ID preparation order.

The reusable bank loader accepts the bounded selected union and existing
AssetSource/AssetDecoder contracts. Retain the 64 MiB encoded read limit and
returned-byte length check, scoped path resolution policies, equal-resolved-key
decode reuse, separately owned PCM per original sample ID, actual bank aggregate
limits, channel conversion and sample-rate validation. WAV00 sharing a resolved
key with a normal/BGM/invisible resource decodes once and retains independent
original IDs. No hidden gain baking, suffix slicing, resampling, path bypass or
special explosion decoder is introduced.

High-level prepare_from_source still refuses actual mines immediately after
parsing and before gain/replay/asset work until complete gauge/fatal-stop and
admission integration. The internal reusable selector/loader can prepare typed
mine PCM for composition fixtures and future admission; this is not a new
playable-file bypass. Preserve no-mine visible/BGM/invisible preparation and
replay validation order through the shared loader. Author independent deferred
fixtures for optional selection and capacity, exact/compatible MemoryAssetSource
resolution and shared decode keys, zero PCM used by actual sound preparation,
missing/oversized/malformed assets and channel/bank limits, and high-level guard
ordering before any resource/decode work. Execution and acceptance remain deferred.

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
