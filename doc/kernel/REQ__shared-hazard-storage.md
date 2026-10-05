# Share immutable hazard and initial configuration storage

HazardTimeline retains validated markers in immutable shared Arc-backed storage.
Cloning a timeline shares its marker allocation rather than copying every marker.
Constructor budget/duplicate-ID checks and stable equal-time declaration order
remain unchanged. markers() still returns a borrowed slice with original IDs,
times, controls and opaque values. Do not share mutable cursor, occupancy or
operation-result buffers between judges or reusable checkpoints.

JudgeEngine retains canonical initial-configuration bytes in immutable shared
storage. Checkpoint clones/from_snapshot reuse that allocation. Preserve exact
canonical encoding and stable hashes, complete custom-state validation, cloned
resolver/policy/interaction ownership, atomic restore/configuration refusal and
last successful hazard reports. All mutable fields remain independently owned.
Retain hazard result capacity and deadline-heap capacity when making checkpoint
copies so template-based construction introduces no additional growth caused by
lost preparation capacity. Do not weaken snapshot validation to optimize copies.

Actual BMS local preparation builds one source-aware pristine judge, then uses
the existing complete snapshot/fork mechanism for additional members. Move the
original into the first member; avoid a redundant template for a single-member
plan. Compile source mine timing once for this member construction and share
immutable configuration/markers across resulting engines. Preserve original
PlayerId order, exact binding coverage/device routes, profile/input-mode checks,
per-player mutable state and disjoint sound voice remapping. No new public engine
fork API or separate judge is required. Preparation remains off the live path.

No-hazard legacy bytes and empty configured timelines retain their original
meaning. Different hazard configuration still refuses restoration; shared memory
identity never substitutes for logical configuration compatibility. Same settings
must produce the same headers/replay identity regardless of marker allocation
address. Public APIs and existing fixture assertions remain intact.

## Evidence and known ceiling

Author core sharing/address, independent state/restore, capacity and byte/hash
compatibility fixtures and actual local preparation/RuntimeGroup member cases.
Use owned canonical bytes and an independent complete-hash framing reference;
do not claim pointer checks replace allocator profiling. Mutable result buffers,
interactions, routing indexes, snapshot validation/serialization and PCM plans
can still allocate or scan. Cold Arc conversion can allocate. No globally
allocation-free checkpoint or measured latency claim is established. Author
assertions for later execution; scoped formatting and four sequential compile-only
checks follow both paired writers stopping. Actual runtime, allocator/benchmark,
platform acceptance, formal review, QA, verify and close remain deferred. Full
BMS player Goal remains active and unproven.
