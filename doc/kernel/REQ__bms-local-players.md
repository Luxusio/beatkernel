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
composition now have source integration. UI roster assignment and multiple
simultaneous native acquisitions remain unimplemented; neither the network
mode nor the execution primitive proves playable local multiplayer.
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
same group through an adapter; multiple device/session/UI wiring is still
required before executable local multiplayer can be claimed.

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
