# Change native output from a retained paused-play screen

An output child screen retains its original live or Watch Play instance and
worker; live play additionally retains its roster, capture and input ownership.
Enter only from acknowledged paused, nonterminal, nonnetwork play whose actual
output adapter advertises control support.
Back returns to the same Play instance and leaves playback paused. Owner finish,
cancellation, focus loss and close settle requests and dispose child UI state
through existing lifecycle rules. Watch exposes this action only for the
composed Linux converted ALSA adapter; unsupported adapters do not expose it. An unavailable F2/open action is ignored without writing a session
failure or changing an existing session diagnostic. Rejected clipboard and IME
edits show an error on the live-output draft while retaining its editor/settings;
a successful subsequent edit clears that local error. Native input remains
independent from text editing in the UI.
Route cleanup uses retained Play ancestry rather than a list of known root
routes. Entering any retained Play child must preserve the game owner and its
background assets. Closing retains the owner until cancellation and join finish.
Request identity and screen-instance identity are independent. A late reply can
update authoritative shared applied settings but cannot overwrite a different
reopened child draft; dispose its field/IME/gesture state using the navigator.
A reopened child whose draft still matches its loaded applied settings may
refresh from a later successful applied capability without adopting the old
child's request identity or notice. Independently edited values/editor and
messages stay intact. Old-child failures do not become the new child's error.

Render the panel with existing retained atoms/molecules, dirty signals and
screen-scoped text/IME/gesture handling. Do not reconstruct Play, mutate unrelated
settings, allocate per note or open output devices on the UI thread. Present
device/output settings and correlated pending/accepted/error results in user
language. Applied settings are acknowledged only after genuine Ready output and
joint clock publication; command admission alone is not success.

Use a bounded, session-isolated output command/reply port, with checked monotonic
u64 request IDs and no reuse. Reject busy/full/closed/unsupported requests before
consuming identity. Preserve queued/in-flight replies through temporary
publication contention; settle each accepted request on cancellation, ordinary
return or owner failure. Fast idle polling uses an atomic pending flag; locks
and allocations belong only to cold commands/publication, never audio callbacks.
Keep the pure command state and statically injected bridge separate from the
PlayerViewer/Publisher synchronization adapter and native request conversion.
Owner unwind must also close the command channel through an actual attachment
guard or authoritative joined-owner boundary. An atomic closed signal allows
settlement to finish after temporary control-lock contention; do not rely only
on the code following the native run closure or overwrite already accepted
applied replies with a later unrelated session failure.
Cancellation rejects new requests and immediately settles commands not yet taken
by the owner. An in-flight command retains its identity until the owner publishes
its decided result or finishes/unwinds; UI cancellation polling must not invent
an earlier result for that command. A decided reply is admitted before cancellation
settlement, so later unrelated owner failure cannot replace it. The Player output
adapter commits decided replies with a cold owner-thread mutex acquisition rather
than a retryable try-lock, preventing a decided result from being stranded in the
bridge when the pump exits. UI commands, polling and request acquisition remain
nonblocking; none of these methods belongs to an audio callback. Ports with
retryable publication retain the existing bridge retry behavior.
Startup capability advertisement is cold owner-thread setup, not an audio
callback. Temporary UI control-lock contention must not fail playback startup;
wait for this bounded critical section and recheck cancellation/closure before
publishing. Cancellation during setup suppresses advertisement without reporting
a gameplay failure; closed noncancelled or poisoned channels still report an
error. Idle UI reply polling reads the atomic busy flag and takes no mutex.

Live requests contain only supported output fields with existing byte/field
bounds. They cannot change chart, keyboard mapping, recording, playback origin,
source sample rate or source channel format. A converted adapter may expose
its supported native target rate, encoding and channel matrix while preserving
the immutable source grid and explicitly refusing incompatible pending PCM.
Linux supports the exact fields advertised by its actual converted ALSA owner,
including endpoint, hardware buffer and processing period. Empty draft values retain the current setting;
an explicit default endpoint remains available. Other native request shapes can
be represented by capability metadata when their owners are composed; do not
advertise unsupported modes or silently fall back.

The native owner receives and validates a request, queues its typed backend
request and waits for the committed pause hook. Retain the actual producer hold
through native readiness and joint clock publication. Existing publication then
releases that hold and leaves producer pause requested; defer coordinated resume
while the correlated reply is still awaiting delivery. Do not extend the
exclusive lease merely for UI result delivery. Invalid settings report a
correlated refusal without retiring the current output. Actual native failure
preserves the original error plus cleanup/recovery state and settles the UI
request. Publish applied size/configuration from the actual native output.

Support periods larger than the initial period within the application's legal
render limit. Raising the Mixer's scalar maximum-render bound at ALSA session
preparation must not allocate a maximum-size default buffer; native scratch stays
sized to the actual opened period. Validate numeric/size constraints before
native retirement, preserving the original PCM format/grid and cursor.

Author command/lifecycle/IME/draft and actual bridge/controller/pump fixtures plus
portable native conversion tests. The user lifted verification deferral on
2026-10-06: execute relevant tests and checks alongside implementation. The
earlier compile-only restriction no longer applies. Final close still requires
actual ordered independent review and required QA evidence. Current REQUEST authorizes disjoint parallel source/test authors; an earlier
session-specific wait-for-another-request statement is historical.
Foreign-call isolation, other OS composition, cross-backend enums and acoustic
acceptance remain full-Goal work; this increment does not close the task.

## Paused Watch output replacement

Records -> Watch -> actual Pause ACK -> F2 Output -> Apply -> Back -> Resume
retains the SAME Play identity, worker and selected recording. Back leaves Watch
paused. Screen ancestry and Results carry the original replay mode; Results
requires owner join. Running, pausing, cancelling, terminal, network and
unsupported sessions ignore Output without changing the session diagnostic.

Reuse the existing complete-output retirement/open/held-start controller and
bounded correlated command port. Watch has no physical input merger or live
input authority. Validate every request before retirement where its facts are
available. Preserve source rate/grid, recorded start/speed/preroll, frozen song,
original operations/hash, actual feeder credit, Mixer and full converter state.
Cold refusal retains the actual attached stream; after retirement, failure
retains recovered/pending ownership and original errors, never claims a closed
old stream was restored.

Prepare pause and applicable finite/idle completion candidates and validate the
actual applied capability before joint output/epoch/pause/completion publication.
Require a fresh held generation and two progressing original native/host pairs.
Reuse AudioAuthorityConfig::default().max_observation_age as a cold replay policy
(currently one second), independent of the request wait deadline: matching
domain, no future pair, age <= policy at readiness AND publication. Old native
completion credit is cleared; genuine source chronology is retained.

The real consumer services replacement before target observation. During the
clock gap it grants no feeder, visual, pause or completion progress while
cancellation remains active. Only successful authorized publication changes its
expected identity. Pending output outcomes continue fencing coordinated resume.
Stop, final native checks and last-source diagnostics handle current output and
proven complete recovered ownership, including cancellation during waiting.
No missing-current error may be invented for a successfully recovered owner.

Existing late-reply, edited-draft, IME, focus, close and request-settlement rules
apply to Watch unchanged. Business owners, real producer/Mixer/converter,
recording/PCM and retained UI paths require executed software tests. Other OS
Watch adapters, browser controls and actual GUI/device/acoustic/performance
acceptance remain original full-Goal obligations, separately reported.
