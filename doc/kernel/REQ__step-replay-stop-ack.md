# Stepped replay stop acknowledgement and output evidence

StepReplay must distinguish immutable planned commands and feeder callback
success from actual remote queue acknowledgement. Reuse OwnedStopEvidence for
the exact prefix of each original retained batch admitted by a valid ACK. Expose
acknowledged_stop_commands() so admission evidence stays readable after technical
failure. The planned feeder admitted_stops count is not this evidence.

Use the existing acknowledge_batch validator and original typed errors. Full
success acknowledges the whole matching batch. A valid unsuccessful ACK retains
its exact admitted prefix, including Stops, before returning the original
AudioRejected error and fencing the owner. Invalid sequence/count/full-success,
unsolicited or repeated ACK earns no evidence. Never retry or count an unadmitted
suffix. Do not clone command vectors or allocate a second prefix for counting;
stage the small checked evidence state and commit only after validator acceptance.
OwnedStopEvidence may be Copy/Clone and expose its count and a bounded unknown-stop
predicate; its private count and checked atomic recording remain authoritative.

For actual output, use application-internal
validate_section_output_evidence_with_stops and completed_render_cursor_with_stops
variants of the existing section output validator and completed_render_cursor. Public generic
validators supply empty evidence and remain strict. Share their existing clock,
capacity, grid, counter, chronology, endpoint and presentation checks; do not copy
the algorithms or normalize raw counters. Only acknowledged Stops may explain
cumulative unknown_stops; it must also fit actual commands_applied. Keep all other
diagnostic semantics and monotonic checks unchanged. Validate every untrusted
report before adopting render, presentation, feeder or visual frontiers.

StepReplay uses its actual ACK ledger in both validation layers. ReplayCompletion
continues requiring genuine recorded-prefix completion, feeder drain, subsequent
idle render and real presentation; finite completion still requires the immutable
endpoint and exact total consumed/applied commands. A Stop allowance alone is no
completion, silence or device presentation proof. Preserve original reports in
typed errors, retained last_render and the read-only admission getter. No browser wire, Worklet
codec, queue/Mixer, live/local/native policy or public report shape changes.

Independent deferred fixtures use actual reconstructed mine-aware StepReplay,
retained batches and real queue/Mixer: planned versus acknowledged Stops, full and
partial failed ACK prefixes, invalid/repeated ACK without credit, output validation
after accepted inactive Stops and strict generic rejection, forged excess/changed/
regressing diagnostics with no frontier adoption, and finite/unlimited completion
barriers with original judge/hash behavior. Worklet/browser/device and performance
execution remain deferred; final clear/fail, native/live output policy and high-level
mine file admission remain unfinished.
