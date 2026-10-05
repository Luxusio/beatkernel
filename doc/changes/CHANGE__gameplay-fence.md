# Portable committed gameplay fence

Runtime now exposes an explicit committed-frontier gameplay fence. Subsequent
physical input and advancement retain original clock/sequence validation and
acquisition chronology, but keep the logical song frontier and judge state
unchanged and generate no bound input, judgment, hazard or gameplay audio. Fenced
input reports retain normalized original physical metadata. Repeated fencing
keeps the first frontier; a pristine owner has no frontier to fence. Queued audio,
held input/contact state, Transport and configured finite-end evidence remain
intact. Explicit paired state restoration clears the fence with chronology.

RuntimeGroup can fence one registered member without poisoning surviving members
or releasing shared queue ownership. Unknown IDs reject before mutation; an
existing technical poison stays poisoned even when a committed prefix is fenced.
SoloRuntime provides the same control and observation facades. These are portable
components with no OS-specific policy or BMS gauge interpretation.

Four independently authored deferred core fixture groups and two local groups
cover actual held/contact/hazard state and queued commands, normalized acquisition
and malformed chronology, explicit state restoration, finite-end and partial
audio-failure evidence, and 2/3/4/64 distinct local members plus solo/poison
behavior. Both writers delivered terminal Writes STOPPED before scoped rustfmt.
Assertions were not run.

Scoped rustfmt and git diff --check completed successfully. Authorized compile-only
checks for workspace/all-target WebTransport, headless/all-target WebTransport,
WASM browser and WASM browser-audio each completed with exit 0. Existing unused
playfield-wrapper and WASM cadence warnings remain. This is compile evidence,
not executed gameplay, hardware, replay-policy or output-drain acceptance.

Known ceiling: automatic gauge failure policy, replay reconstruction, input-mask
presentation cleanup, clear/fail completion and actual per-player audio stopping
remain unfinished. This explicit kernel component does not assert finished or
drained output and does not remove the high-level mine admission guard. Tests,
applications, devices, browsers, measured performance, formal review and QA remain
deferred.
