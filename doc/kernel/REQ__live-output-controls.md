# Change native output from a retained paused-play screen

A live-output child screen retains the live Play instance, worker, roster,
capture and input ownership. Enter only from acknowledged paused, nonterminal,
nonnetwork live play whose actual output adapter advertises control support.
Back returns to the same Play instance and leaves playback paused. Owner finish,
cancellation, focus loss and close settle requests and dispose child UI state
through existing lifecycle rules. Replay and unsupported adapters do not expose
this action. Native input remains independent from text editing in the UI.
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
Startup capability advertisement is cold owner-thread setup, not an audio
callback. Temporary UI control-lock contention must not fail playback startup;
wait for this bounded critical section and recheck cancellation/closure before
publishing. Cancellation during setup suppresses advertisement without reporting
a gameplay failure; closed noncancelled or poisoned channels still report an
error. Idle UI reply polling reads the atomic busy flag and takes no mutex.

Live requests contain only supported output fields with existing byte/field
bounds. They cannot change chart, keyboard mapping, recording, playback origin,
sample rate or channel format. Linux initially supports ALSA endpoint, hardware
buffer and processing period. Empty draft values retain the current setting;
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
actual ordered independent review and required QA evidence. Current session
delegation follows the user's instruction to wait for another parallel request.
Foreign-call isolation, other OS composition, cross-backend enums and acoustic
acceptance remain full-Goal work; this increment does not close the task.
