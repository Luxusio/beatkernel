# One-shot hazards in the shared judge

Configure an optional immutable `HazardTimeline` once before the first accepted
JudgeEngine input or advance. Each marker carries a unique full u64 `HazardId`,
signed song timestamp, logical control and opaque u64 application value.
Validate duplicate IDs and the caller's marker budget before installation;
sort by timestamp preserving declaration order at equal times. Preparation may
allocate; retain a cursor and preallocated operation-result buffer rather than
scanning all markers or allocating hazard results on every gameplay operation.

Use the engine's actual button and enabled touch-contact ownership. Track only
hazard controls; duplicate Down, Repeat, orphan Up and unrelated controls cannot
change occupancy. Multiple sources, physical controls and contacts contribute
independently. Release/Cancel removes only its actual owner. A control remains
occupied until its last owner releases. Move and axes do not acquire occupancy.

After successful time/resolver preflight, input processes markers strictly
before its effective time using previous occupancy, commits ownership, then
processes equal-time markers with resulting occupancy. Advance consumes markers
through its effective time. Each marker emits exactly one Triggered or Avoided
outcome. The first successful operation at an equal-time boundary consumes it;
later operations never reevaluate it. This explicitly preserves operation order:
an initial Down can trigger, an initial last-owner Up/Cancel can avoid, and an
advance before Down can avoid. Times use the existing checked judge input-offset
policy for both input and advance; retain the marker's original time in output.
Only markers exactly at an input operation's effective time carry that operation's
original EventMeta. Do not substitute arrival time or invent metadata for advance.

`hazard_events()` borrows the last successful operation's hazard outcomes. A
successful operation replaces that report; rejected operations preserve report,
cursor, ownership and complete state. Existing normal JudgeEvent returns remain
unchanged. Gauge, death and sound interpretation belong to the application.

Snapshot/hash include immutable hazard configuration, cursor, occupancy and
retained outcomes with complete provenance. Compatible restore reproduces future
outcomes and preserves result capacity; different configuration refuses atomically.
No configured hazards preserves existing canonical bytes and gameplay semantics.
Configuration failure is atomic, including unsupported custom snapshot policies.

Author deferred fixtures for ordered boundaries, independent owners/contacts,
rejected operations, normal judgment coexistence, long signed times/overflow,
restore/reusable checkpoints/config mismatch and default compatibility.

## Known ceiling

JudgeEngine processes hazards and RuntimeReport now delivers actual successful
call outcomes. BMS source-plan/judge construction now shares the actual stepped,
local-member, source-aware replay and offline composition paths;
consumer-side live/local/replay/practice/offline gauge/death, WAV00, completion
and rendering remain required integrations. Keep the shared BMS mine
admission refusal until those owners actually consume hazards. Compile checks
and authored fixtures do not prove executed browser/device/performance acceptance.
