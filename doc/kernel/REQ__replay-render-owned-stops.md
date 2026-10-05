# Replay PCM rendering and actual owned stop admission

The recorded audio planner now emits Play and gameplay-failure Stop commands.
render_replay owns a fresh queue and feeds only this validated plan, in its stable
target-frame order. It must render legitimate inactive or expired gameplay Stop
targets without treating their existing unknown_stops telemetry as unexplained
audio failure. Keep all raw render counters and all other strict diagnostic errors.

Share a small application-internal OwnedStopEvidence component with synthetic
offline rendering, rather than duplicate numeric allowance or render checks.
Its private cumulative count starts at zero, counts only Stops in commands whose
actual queue admission succeeded, and uses checked arithmetic. Owners record each
accepted prefix once; Play, planned/requested commands and rejected commands earn
no Stop allowance. record_admitted accepts an actual accepted command slice and
returns an overflow error without changing prior evidence. This records queue
admission, not per-command execution, physical silence or completion.

Use one shared render_block_with_stops path taking this evidence. Generic
render_block always supplies empty evidence and continues rejecting unknown_stops.
The shared checker permits only cumulative unknown_stops <= actual admitted
Stop count; late/pending/voice/sample/gain/rate/time diagnostics remain failures.
Never normalize or hide counters in returned reports or error evidence.
The two callers establish the closed queue invariant: synthetic BGM is Play-only,
synthetic Stops come from actual RuntimeSoundStopReport; replay commands come
from the validated planner and its private producer has no external writers.
Do not expose this policy as a public general-purpose validator or core API.

render_replay records a Stop only after producer.try_push succeeds. Keep exact
original admission failure and already written PCM/latest render evidence; never
retry or count the rejected command. Preserve commands_admitted as the full actual
Play/Stop prefix count. Respect the caller's exclusive output frame extent,
including all same-time command ordering, preroll, and excluded Stops. Preserve
full-log judge results/hash after legacy gauge failure independent of PCM cutoff.
Numeric failure stops gameplay sounds while independent BGM continues. No extra
grading, global pause or output truncation. Public report shapes remain unchanged.

Independent deferred fixtures cover actual reconstructed fatal legacy replays,
PCM and BGM preservation across block sizes, inactive Stop telemetry and full-log
counts/hash; partial Stop rejection with written-prefix evidence; finite requested
output cutoffs/preroll and no-mine legacy behavior; and shared evidence classification
against actual queues/Mixer, including generic strict rejection and other errors.
Keep existing synthetic offline fixtures unchanged. Compilation and authored
fixtures do not establish assertion, browser/device or performance acceptance.
Native playback, Worklet acknowledgement, owner completion and final clear/fail
remain separate integrations, and high-level mine admission stays guarded.
