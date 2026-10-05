# Replay gauge failure sound planning

Share the scheduled voice-stop state with actual Runtime execution rather than
copying its sorting, watermark and one-attempt rules into the replay planner.
Expose GameplaySoundStop in beatkernel::runtime with a caller-bounded prepared
sorted unique voice list, accepted/planned output watermark, one-attempt latch,
callback attempts and explicit reset. Keep RuntimeSoundStopReport reachable at
its existing public path. Runtime retains its actual committed-fence requirement,
real producer/telemetry callback, setup locks and paired restoration behavior.
The pure component owns no judge, queue, transport, assets or BMS policy.

An admitted timestamp means the caller accepted the relevant gameplay command:
Runtime records actual successful queue admission, while the replay planner
records inclusion in its off-thread plan. Callback/planning success never proves
later queue admission, Worklet acknowledgement, mixer execution or silence.
Preserve all unique-voice Stop attempts and original callback errors, first stop
time max(requested, watermark), no implicit retry and reset only by explicit owner
restoration. Do not change queue codecs, Mixer semantics or replay setup bytes.

For every mine-containing source, reconstruct one pristine source-aware selection
judge even when WAV00 is missing, fatal-only or has no audible hazard bindings.
Run every actual recorded operation in order, retaining the complete reconstructed
judge results/hash, including later legacy operations after numeric failure.
Use the same default BmsGauge as live owners and observe actual normal/hazard
reports once per operation. No mine marker or missing sound alone implies failure.

Before first numeric failure, select normal/press/hazard sounds as today. Preserve
all sounds from the failure-causing operation, then prepare one Stop per unique
normal/press/hazard voice at the mapped failure operation time clamped after all
already selected gameplay output times. Preserve BGM and suppress only later
gameplay sounds after failure; no voice Stop is synthesized for BGM. Reject a
failure stop namespace that collides with prepared BGM voices rather than claim
that stopping a shared voice preserves its BGM. Healthy/nonfatal/recoverable-zero
plans keep their existing audio behavior.

Apply section start, judge offset and output preroll exactly once. Preserve
background-before-gameplay and original same-time command order. Respect the
existing exclusive finite endpoint, including Stop commands whose rounded frame
reaches it. Map a failure timestamp only when there are voices to stop; an empty
silent failure must not invent an output mapping requirement. No-mine plans keep
their existing selection path and byte-identical command ordering.

Expose the same ReplayAudioPlan shape, documenting output Play/Stop commands.
The mapped feeder already supports these Stops; actual owned Stop diagnostic/
Worklet acknowledgement tracking remains a separate integration. Do not weaken
generic strict render validators or hide unknown_stops. The separate
[offline runtime owner](REQ__offline-gauge-sound-stop.md) observes actual reports
and tracks admitted Stops in its fresh queue. Failed-session native/replay output
completion, clear/fail and high-level mine admission remain unfinished.
Compilation and authored fixtures do not establish
assertion, browser/device output or performance acceptance.

Independent deferred fixtures cover the actual shared component with partial
callback errors/reset and actual mine-aware replay plans: audible/non-audible
fatal failure, preserved failure Play prefix, subsequent legacy hash/results with
suppressed sounds, BGM exclusion/collision, offsets/preroll/finite endpoint,
recoverable/nonfatal/no-mine legacy behavior and actual feeder/Mixer Stop evidence.
