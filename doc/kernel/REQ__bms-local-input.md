# Bounded local input merge frontier

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
actual native acceptance remains deferred by the user.
