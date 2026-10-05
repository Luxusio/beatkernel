# Native numeric-failed terminal readiness

Native solo and local gameplay use actual gauge failure plus the owner's committed
Runtime fence as terminal gameplay readiness. Pending notes/hazards, judge hash,
score, capture prefix and frozen song time remain intact. A control fence alone,
recoverable empty gauge or technical observation/admission error cannot establish
successful completion. Healthy local members must still complete normally.

Keep the common native device interface and platform launchers unchanged. Record
only newly returned fence-gameplay-sounds commands which the actual producer
accepted, exactly once; never count the whole republished report, rejected Stops,
planned voices or a repeated stop attempt. Reuse internal OwnedStopEvidence.
Original partial reports and independent errors remain observable on failure.

Before completion adopts native mixer evidence, unknown_stops must not exceed
actual admitted Stops or commands_applied. Preserve raw counters and established
clock/grid/pause/endpoint/presentation checks. Admission proves queue acceptance,
not physical presentation. No global generic-validator relaxation is permitted.

Unlimited failed members use the existing SongCompletion terminal-ready helper
and shared BGM/drain/presentation machinery. Every healthy member still passes its
normal prepared deadline, notes and hazards; all-failed cohorts may drain at their
frozen song times. Finite failed readiness replaces only its frozen song-end check:
the actual NativeEnd boundary, committed host frontier, input draining, resume
guards and existing endpoint evidence remain required. Numeric-failed finite
completion additionally waits for empty BGM credits and pending-free output with
the actual paused, immutable endpoint marker. The Mixer retains active voice state
at a finite endpoint while zeroing all subsequent output; do not require those
retained BGM tails to disappear. Genuine NativeEnd presentation proves the finite
cutoff. Unlimited drain still requires inactive voices and later presentation.
Expose an internal read-only accepted-audio-command count from the actual shared
CommandProducer through RuntimeGroup and SoloRuntime. Numeric-failed finite
completion requires exact commands_consumed and commands_applied equality with
that producer count, including initial BGM and every member's actual admissions.
Reject ambiguous saturated admission counts. This proves newly admitted Stops
were processed even when the physical endpoint report was captured concurrently
with admission; an endpoint with commands still stranded in the producer cannot
be claimed completed. Do not substitute report-vector counts or sum duplicated
telemetry/BGM counts.
An idle render observed before the newly admitted Stops cannot establish this
completion. After admission, retain the end of an actual native render report and
require a subsequent nonempty block starting at or after that end. If no report
is available then, the first later nonempty report establishes the barrier only.
Reset unlimited SongCompletion drains when new Stops are admitted. Neither path
fabricates an output block, clock point or presentation.
Keep ordinary public finite helper behavior for healthy callers.

Finite native audio preparation excludes BGM cues whose mapped target frame is
at or beyond the exclusive playback endpoint before any producer admission.
Use the existing exact relative-song/preroll frame mapping; preserve equal-time
order and reject invalid cues as before. A stopped finite playback cursor cannot
retire future excluded cues, so they must never become pending BGM credits.
Do not manufacture retirement or output counters at the endpoint.
The real endpoint render report is paused after its final active prefix. Retire
BGM credits using its actual completed logical playback cursor when an explicit
playback_end_physical_frame is present; ordinary manual pause/startup silence
still cannot feed BGM. Preserve the existing Late error for any missed valid cue.

Independent deferred fixtures exercise actual fatal solo/cohort operations and
real software queue/Mixer evidence: future notes/hazards, failed primary with
healthy survivors, all-failed cohorts, BGM and idle/presentation barriers, finite
boundaries, partial Stop refusal, unowned/excess unknown Stops and repeated reports.
Tests, device/browser/performance acceptance and formal review/QA remain deferred.

Final clear/fail classification remains separate. Preserve the high-level mine
admission guard until native output and final-result integration is finished.
