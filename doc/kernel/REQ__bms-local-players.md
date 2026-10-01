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
simultaneous evdev sources. UI roster assignment and Windows/macOS multi-input
acquisition remain unimplemented; source integration does not prove native
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
devices with this group; GUI and other native platforms still require wiring.

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

This integration starts with Linux terminal play. Graphical multi-player
presentation/assignment and Windows/macOS native multi-input integration remain
required work. GUI multi-input and combined local-network sessions currently
fail explicitly before resource acquisition rather than claim partial support.
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
