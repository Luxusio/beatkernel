# Bounded local input merge frontier

## Fixed and dynamically acquired source identities

Keep the existing fixed 1..64-device constructor for configured local groups.
An explicit dynamic constructor may start with no sources and reserve a caller
selected 1..4096 source slots, preserving the existing Windows any-source range.
Register the actual ID carried by native acquisition before admitting its event;
never invent aggregate IDs or force a one-player device selector. Known source
registration is idempotent and retains its last timestamp/sequence. Fixed mode
rejects unknown sources; dynamic saturation rejects before any state mutation.

Reserve source and pending storage at setup. Source lookup is ordered/binary;
registration must not grow the allocation. Retain registered source chronology
through removal rather than recycle identities or erase sequence history.
The source bound is cumulative for a session, separate from the existing
64-MiB pending heap/blob budget. Each explicit registration/admission operation
is atomic; rejected events cannot reset an existing source's history.

For [audio-authoritative playback](REQ__audio-authority.md), this merger still
owns original HOST acquisition and the fully processed prefix. Runtime's judged
deadline belongs to the separately admitted logical audio frontier. Drain input
before closing the full HOST prefix; a host timer or generated render cursor
does not grant an audio deadline. Preserve the global backlog barrier across
every source. Generic legacy HOST-domain consumers retain the contract below.

Gather input from all local devices on the native game owner before releasing
ordered events to RuntimeGroup. Keep original event/native metadata and host
timestamps; never replace late input with the UI frame time or newest clock.
Register distinct devices in one host domain. Bound the pending queue to at most
65536 entries and 64 MiB retained event/blob storage. Overcapacity fails explicitly.

Order by host timestamp, source identity, acquisition sequence and stable admission
ordinal. Within a source, timestamps and sequences may stay equal but never
regress. Reject unknown domains/devices, future timestamps and events at/before
an already committed deadline frontier. Setup origin excludes older events.

Compute a frontier from fresh host time minus caller-supplied 0..1s lag using
checked arithmetic. Any backlogged device blocks frontier advancement. Pop all
pending events through the selected frontier, process them, then advance all
member deadlines at the same point and commit only with no older input pending.
Never advance first and quietly feed expired input afterward. Commit errors
preserve the previous accepted frontier. OS latency beyond the configured lag
can still produce a late-input error; this is not proof of zero physical drift.

Linux round-robin acquisition checks every node, handles bounded batches and
stops on synchronization loss/disconnect. Input reads, merge/group execution and
output preparation are outside audio callbacks and UI event timestamping.
Fixtures and compilation do not establish executed ordering under native load;
physical input/audio acceptance remains unproven and requires actual execution.
