# Local player roster and input ownership

Windows groups use repeated stable `--local-player ID:INTERFACE_PATH` assignments
and the same graphical roster as Linux. Each exact keyboard resolves to a fresh
Raw Input DeviceId/handle before output starts; aliases or detached assignments
fail rather than substitute a device. A single game-owner message pump acquires
all keyboards using original QPC receipt metadata and routes each to its own
actual Runtime. One transport/output/BGM is shared. A bounded merger freezes
deadline advancement while messages remain queued; `--advance-lag-ns` (0..1s,
default 2ms) controls its common frontier. Selected removal and chronology errors
stop the whole group, preserving completed prefixes through cleanup and replay
save. Raw Input receipt timestamps do not establish physical key actuation time.
Native execution acceptance remains deferred. macOS groups use exact positive
IORegistry identities with one IOHID owner and shared CoreAudio output; normalized
Mach event timestamps feed the same bounded merger and independent member state.

Each stable local member owns its competition presentation: at most eight
recorded ghost prefixes and one peer-reported prefix, with connection lifecycle
retained through cleanup. Group reports update the matching PlayerId only.
Saved opponents may be compared for each local member; combining a local group
with the existing two-peer network mode still fails before resources start.

A solo player starts without input-device selection. Two or more local players
assign distinct input devices to prevent one physical event from playing every
chart. Support a collection of players, including three and four; do not encode
only P1/P2, fixed two-player fields or a four-player array. A caller-supplied
resource capacity limits the roster (up to 64), independently of game layout.
This is same-host local play, separate from existing network peer competition.

### Resolved source plan shared by native and browser hosts

After host acquisition, represent routes using positive stable PlayerIds and
canonical full-width DeviceIds, with no operating-system tag, path, product
description or permission handle in the shared plan. Retain member order and
own the immutable route snapshot. Validate one to 64 unique player identities;
every member in a multi-player plan needs a distinct exact source. A solo member
may retain automatic routing or an explicit source. DeviceId zero is a valid
canonical identity; host-specific reserved-source rules belong to acquisition.

The bounded numeric host bridge uses four u32 words per member: PlayerId,
selector (0 automatic, 1 exact), source low word and source high word.
Automatic rows require both source words zero and are legal only for solo.
Reject empty/partial/over-capacity rows, unknown selectors, zero/duplicate
PlayerIds, duplicate exact sources and automatic multi-player members. Preserve
all 64 source bits without narrowing through floating-point numbers.

Native identity resolution validates its entire assignment draft before calling
attachment lookup, then applies the same canonical source validation used by
RuntimeGroup setup. Lookup failure retains its real side-effect prefix; the
shared plan does not open, close or roll back host devices. RuntimeGroup still
requires exact member binding selectors and disjoint voice ownership. Source
validation belongs to setup and adds no per-input or audio-callback work.

Browser local bindings and the Worker now use this shared bridge, including
paged multi-field rendering. The browser page now connects discovered source
assignment and per-member record save/download selection in source. Actual
browser/device/runtime acceptance remains deferred.

### Common prepared members

Prepare members from the actual PreparedBms, resolved source plan, one supplied
BindingMap per member in retained plan order, an explicit JudgeProfile and
BmsInputMode. The common builder owns no files, native handles, permissions or
settings hosts. It uses the existing compiled chart, BMS interaction rules and
VoiceAllocator; it does not reparse charts, decode assets, copy PCM or create
another judge implementation. Each member gets independent existing judge
state and sounds; the original SampleIds and PCM bank remain shared.

Validate binding-map count, exact source selectors for assigned members and
coverage of the chart's lane controls before creating members. Scan source
notes once to collect the at most eighteen distinct lanes, then validate each
member against that bounded set rather than rescanning every note per member.
Coverage here
means configured controls; host-specific Button/Axis/Touch admission remains
the acquisition/setup contract. Automatic solo can retain Any/exact bindings.
Reject nonfinite SoundBinding gains and non-Play prepared BGM commands. BGM
Play gain validation remains with the existing downstream BgmFeeder. Reserve the
actual BGM voices, allocate disjoint per-member key-sound voices in member order,
and preserve same-member voice replacements and all other sound fields.
Allocation/namespace failure exposes no partially prepared cohort.

Native cohort preparation uses this same builder with ButtonOnly rules and
its existing keyboard maps. Host-owned capture/completion/opponent loading
remains outside the common builder. Existing native source/device constraints,
recording identities and voice order must be preserved. Browser group ownership
now uses this common prepared member layer. The browser page assignment and
per-member storage callers are implemented in source; playable acceptance
remains unverified.

### Nonblocking shared local gameplay owner

StepLocalGameplay consumes actual PreparedBms, the resolved source plan,
ordered BindingMaps, StepGameplayConfig, immutable section start/end and
BmsInputMode. Use the existing common member preparation and RuntimeGroup.
Return one original PCM bank to the host. Share StepGameplay's BGM scheduler,
output clock discipline, command consumer, pending batch/ACK, finite endpoint
and real output completion implementation; do not create another output queue,
copy the producer, sample a host clock or implement a second evaluator.
Existing public solo constructors, reports, replay bytes and command order stay
compatible, including original solo SoundBinding voice IDs. A newly constructed
local owner, even with one member, uses the common prepared-member namespace;
semantic PCM and replay do not require its numeric voices to match legacy solo.
A private shared controller must not expose scalar solo gameplay
operations for a multi-member runtime.

Retain stable member order, independent score, last logical song time and
optional replay capture. Route original input, including projected contact, to
its exact member and expose actual InputResult/PlayerReports. Unknown sources
remain ignored without changing setup readiness. One deadline advance uses
the same host/output points for all members. Preserve all committed reports
on core, scoring or capture failure, including later reports already committed
by the same group operation; fence the entire owner without rollback/retry.

Configure per-member capture, competition identity and contact routing against
the actual pristine judge. Identity/capture uses the same section/input-mode
codec as solo; export each capture once only after explicit stop/failure.
One output-clock correction applies to the group's shared Transport after a
successful common advance. Setup refusal does not corrupt accepted state.

Completion always validates genuine output evidence. Unlimited play requires
every member's actual chart interactions complete as well as existing shared
audio drain/presentation barriers. Finite play requires each member's logical
position at the immutable endpoint, genuine configured Mixer fence, resolved
queue/BGM credits/ACK and exact executed acknowledged command counts.
Pending ACK never stops judging; partial or rejected ACK fences the whole owner
and retains the real admitted prefix without replaying its tail. Source/compile
and authored fixtures are not runtime acceptance. Browser bindings and Worker
renderer integration, page assignment and per-member record callers are
implemented in source, including per-member saved comparisons. Local network
competition and browser acceptance remain requirements; touch page remapping
is implemented in source.

Player IDs survive roster growth and shrink for retained members. A solo roster
uses automatic input and clears previous explicit assignments. Each member of
an N>=2 roster must have a unique nonempty bounded native input identity before
session routes can be sealed. Duplicate assignments, invalid identities, foreign
players and capacity violations fail without mutating accepted state. Resolve
native identities to session DeviceIds separately at actual native preparation;
never persist discovery-time runtime IDs or substitute missing attachments.

Each player requires independent binding/judge/replay/score state while sharing
the same song transport origin and output frame mapping. Physical events route
to exactly the owning player. Shared BGM is scheduled once; player key sounds
need collision-free voice identities. UI layout consumes the roster rather than
assuming two panes. Removing a member/session route needs held-key cleanup.

Portable roster/admission, solo automatic preparation and shared Runtime
composition now have source integration. Linux terminal composition adds multiple
simultaneous evdev sources with graphical roster assignment. Windows composes
simultaneous Raw Input sources; macOS composes simultaneous IOHID sources.
Source integration does not prove native
multiplayer acceptance.
Actual discovery, filtering and multi-player acceptance remain deferred.

## Shared Runtime composition

Use the existing core Runtime for each player's binding/judge/replay sequence
state. One group owns the authoritative Transport and actual mixer producer.
Temporarily exchange those owners into the routed member and restore with RAII,
including unwinding. This preserves the unique SPSC producer without another
audio queue or per-step cloned transport. Placeholder owners are setup-only.
Never execute a member outside the group while placeholders are installed.

Route exact input to one member; unassigned sources do not advance gameplay.
Advance deadlines for every member using the same supplied host/output points.
Reports retain player identity and actual core result/failure data. A partial
advance/input failure makes the group unusable for further play; published
audio/judgments are not rolled back, and prior member reports remain inspectable.
No caller may resume gameplay after unwinding or partial failure. Explicit
output queue admission and transport inspection remain available for host
cleanup; neither operation removes the gameplay failure fence.

Validate member limits/identities, exact multi-player binding selectors and
disjoint player/BGM voice identities during setup. Checked namespace allocation
happens outside real-time callbacks. Existing native solo compositions use the
same group through an adapter. Linux terminal local sessions compose multiple
devices with this group and publishes the same member reports to graphical
panels. Settings Players supplies the native roster; Windows and macOS source
paths likewise acquire multiple exact inputs as described above. Native
execution acceptance and actual browser gameplay acceptance remain pending.

Group telemetry retention is limited to 65536 samples per member and 1048576
samples in aggregate; zero retains counters only. Each member's judge/history
allocations remain separately supplied prepared state. Retained members cannot
share sound voices with another member or caller-reserved BGM voices. The
off-thread allocator preserves repeated old voices within one member, commits
remapping only after checked allocation, and fails identity exhaustion without
partially changing sound mappings. Shared BGM is admitted through one producer.

The solo adapter returns an actual partial RuntimeReport to existing replay/UI
consumers before the gameplay fence rejects a subsequent operation. Multiple
member failure retains every completed report in GroupError; published results
and commands are never silently removed or replayed. These fixtures are authored
only; executed validation remains user-deferred.

## Linux native local session

Repeated --local-input PATH selects 2..64 local keyboard nodes in player order,
separate from automatic solo mode; --evdev cannot be combined with it. Reject
aliases resolving to the same physical character-device number. All evdev
owners use one monotonic domain and maintain their original native timestamps.
A bounded merger orders admitted input before the common deadline frontier.
Each player has its own judge, replay capture, ghost comparison and completion;
one PCM bank/mixer/BGM scheduler/output and presentation mapping are shared.
Stop the cohort on synchronization loss, disconnect, late input or partial
group failure. Only after all judges and shared output drain finish is the
cohort complete. Save separate .p<ID>.bkr recordings after native cleanup.

This integration starts with Linux terminal play and its graphical publication.
Graphical roster assignment and native multiple-input composition have Linux,
Windows and macOS source integration. Combined local-network sessions currently fail explicitly
before resource acquisition rather than claim partial support.
Native execution and file/recording acceptance remain deferred.

## Known ceiling

The caller's shared mixer voice budget remains 1..4096 (default256), and the
rolling BGM feeder reserves 1024 command slots for live admission. A64-player
roster does not prove worst-case simultaneous chords fit either audio budget.
Group command-admission failures stop the cohort; native mixer voice-limit
outcomes remain visible in mixer telemetry. Judge, replay and ghost memory
scale per player; only the PCM bank, BGM schedule and output are shared once.
The pending merge bound excludes allocator bookkeeping and an in-flight or
rejected event owned by the caller. Configured lag cannot bound arbitrary OS
input delivery latency; late input fails explicitly.

## Local-player presentation

The existing latest-state channel carries a stable-ID collection of1..64 local
members, each with exact prepared chart/time and independent actual score and
recent128 judgments. Shared charts use one Arc; terminal status/cancellation
belongs to the session and retains all members through native cleanup. Solo
calls keep their existing shape and populate one member. Unknown/duplicate
report identities fail before application; display coalescing never affects
core judgment/replay or audio admission. No summed score represents the group.

Graphical play draws2..4 independent panels on one page, and larger rosters in
pages of at most4 members. PageUp/PageDown and explicit page buttons remain
available while playing and on results. Page changes are UI-only; native input
continues for every member. Notes and text stay within each panel and retain
original song-time projection. Prepared-chart and report publication occur on
the game owner, never audio callbacks or UI event timestamps. Linux local mode
attaches this presentation; Settings Players assigns devices or imports CLI/profile
identities. Windows and macOS use the same presentation.
Native UI/GPU/execution acceptance is still user-deferred.

## Graphical roster assignment

The product is the graphical BMS player. Native terminal commands are developer
compositions, not the intended rhythm-game interface. Settings Players opens a
bounded local roster draft, with count increase/decrease, player selection and
paging up to64. A single player keeps automatic input and does not show a device
chooser. Each of2+ players needs a distinct explicitly assigned keyboard.
Assignment uses the existing metadata worker and typed paged keyboard catalog,
never UI timestamp acquisition. Query/refresh is serialized; pending controls
and hidden text fields are fenced. Disabled devices cannot be assigned and
already-used identities fail without changing the prior accepted draft.

Done requires a sealed valid roster and changes only the settings draft; Apply
sets the next session. Back discards local changes. Player IDs remain stable
through retained members during resizing and imported profiles. Native Linux
--local-player ID:PATH repeats preserve those IDs; legacy --local-input PATH
retains sequential IDs. The two forms and solo --evdev cannot mix. Native fresh
DeviceIds stay separate from stable player IDs and opened alias/availability
checks remain authoritative. Returning to solo clears local assignments.

Current multiple-input source composition and assignment support covers Linux,
Windows and macOS. macOS --local-player ID:REGISTRY uses positive decimal u64
registries and rejects numeric aliases; group/solo registry overrides cannot mix.
Fresh native attachment IDs remain separate from persistent player IDs. Audio
configuration remains one shared optional advanced override. Profile value and
aggregate settings caps remain in force, including ID/path encoding overhead.
GUI rendering, metadata query and actual play acceptance remain user-deferred.

Linux explicit profile overrides replace the entire input assignment family
(--evdev, --local-input, --local-player) so selecting one form removes stale
profile entries from the others. Conflicting forms supplied together remain an
explicit native validation error. Within an accepted draft, exhausted player-ID
space still permits retaining/shrinking existing u32::MAX IDs; growth fails
atomically rather than reusing retired IDs.

### Native finite local cohort endpoints
WASAPI shared/exclusive and CoreAudio local 2..64 sessions accept --end-ns with one shared immutable audio/logical end. Finish only after actual native presentation, every acquisition source drained, real globally committed input frontier past the terminal boundary, all members at logical end and pending resume reconciliation complete. Preserve per-player original-input prefixes and independent captures/scores; do not force unfinished notes. ASIO/network finite integration remains pending. Fixtures must be prepared for later execution; source compilation is not native acceptance.

### Browser page local assignment and records

The browser page must keep solo automatic and expose source assignment only for
two or more players. Retained players keep positive stable IDs when resizing;
growth never reuses retired IDs. A session snapshots a one-to-64-member plan
before asynchronous preparation. Multi-player rows use distinct full-width
acquired source IDs: keyboard source 1, touch source 2, or owned HID/Gamepad
attachments. Product descriptions are display labels, not route identities.
Missing, disconnected, duplicate or unconfigured sources refuse preparation.
The roster limit does not promise 64 browser devices: current acquisition caps
HID and Gamepad at 16 each, plus one keyboard and one touch aggregate. Actual
source availability and the combined 256-binding-row budget may impose lower
limits. Browsers cannot assign separate keyboards from the aggregate events.
Window acquires input and filters unassigned sources before enqueueing, retaining
original timestamps, sequence numbers, contacts and provenance. Worker performs
binding, judgment, score calculation and paged OffscreenCanvas rendering.
Discovery retains the actual acquisition owners until adoption or cleanup;
adoption never reallocates selected identities or resets acquired input sequence
numbers. Overlapping discovery is refused. Cancellation, focus loss, library
reset and page teardown fence asynchronous discovery before releasing all owned
devices and listeners. A cleanup failure prevents further playback until reload.

Each captured member has an independent validated replay, completion label,
score and export error. The page must offer explicit member selection for
download or library save, retaining other valid prefixes when one export fails.
Local playback must explicitly refuse unsupported network combinations and
invalid saved-record targets before acquiring audio. Page changes preserve Worker's held-contact ownership while remapping visible
geometry and disabling new contacts for hidden touch fields. Authored fixtures and source inspections
are preparation for deferred browser acceptance, not execution evidence.

### Browser local saved-record competition

Each selected saved record in local play must name an actual stable member ID.
One record belongs to one comparison target; never implicitly duplicate records
across all members or choose the first member. Keep the existing aggregate eight
record and 64 MiB limits across the whole local session. Own/Other and labels
remain display choices. The target member's pristine competition header admits
compatibility; comparison advances only through that member's actual song
frontier. Saved judging never changes live judgment, captures or shared audio.

Worker owns saved-prefix advancement and retained per-member HUD snapshots.
Final results preserve each record's member ownership and independent comparison
failure. A failed member comparison disables only its own HUD and retains other
members' results. Removed or unknown member targets refuse before consuming
preparation. Solo keeps its existing untargeted comparisons; targeted local
selections must be explicitly reassigned or cleared before solo playback.
Reserve each member's comparison space after admission and before contact setup.
Rendering and touch routing use the same shifted field bounds. Keep that space
after a HUD failure; a disappearing comparison must never move a live touch lane.
Actual browser/device/runtime acceptance remains deferred.
