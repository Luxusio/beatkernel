# Local player roster and input ownership

A solo player starts without input-device selection. Two or more local players
assign distinct input devices to prevent one physical event from playing every
chart. Support a collection of players, including three and four; do not encode
only P1/P2, fixed two-player fields or a four-player array. A caller-supplied
resource capacity limits the roster (up to 64), independently of game layout.
This is same-host local play, separate from existing network peer competition.

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
simultaneous evdev sources with graphical roster assignment. Windows/macOS
multi-input acquisition remains unimplemented; source integration does not prove native
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
panels. Settings Players supplies the Linux roster; other native platforms still
require multiple-input wiring.

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
Graphical roster assignment has Linux source integration. Windows/macOS native
multi-input integration remains required work. Combined local-network sessions currently fail explicitly
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
identities. Windows/macOS multi-input remain required later work.
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

Current multiple-input execution/assignment support is Linux; WinMac count
growth explicitly reports unavailable acquisition until implemented. Audio
configuration remains one shared optional advanced override. Profile value and
aggregate settings caps remain in force, including ID/path encoding overhead.
GUI rendering, metadata query and actual play acceptance remain user-deferred.

Linux explicit profile overrides replace the entire input assignment family
(--evdev, --local-input, --local-player) so selecting one form removes stale
profile entries from the others. Conflicting forms supplied together remain an
explicit native validation error. Within an accepted draft, exhausted player-ID
space still permits retaining/shrinking existing u32::MAX IDs; growth fails
atomically rather than reusing retired IDs.
