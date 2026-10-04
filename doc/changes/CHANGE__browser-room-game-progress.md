# Actual browser room gameplay progress

Connect actual local gameplay progress to the Worker-owned room client, retain
accepted participant-specific peer prefixes inside the Worker, and distinguish
local publication admission, full write and aggregate acknowledgement in the
terminal room outcome. Preserve local gameplay when a room connection fails
after activation; pre-start failures still cancel preparation.

The Worker now publishes actual ordered member words after successful gameplay
steps and output observations. It checks the existing 250 ms cadence before
acquiring words, preserves the Prepared roster and retains only the latest
accepted DTO per peer in Prepared order. Genuine completed output and pre-
disposal final publication bypass ordinary cadence once. The terminal `room`
field exposes queued, written, acknowledged and local-complete flags separately.
Stop releases the game/captures first and joins channel continuations before
publishing its terminal outcome.

Post-activation room faults fence further publication and close the room owner
while local input, output and recordings continue. The Window updates connection
status once; malformed initial control metadata and pre-start failure still
cancel preparation. Final status distinguishes a queued prefix from actual
full-write and relay acknowledgement evidence. No Window rendering or periodic
score update loop is added.

Independent deferred fixtures add four Worker groups (88 total) and one Window
group (93 total), preserving prior groups. They cover cadence before word
acquisition, exact ordered/full-width values, pending-start peer retention,
final receipt barriers, joined cancellation, active local continuity and stale
cleanup. Full-width scripted counts preserve common counter validity. Fixture
source and whitespace were inspected; no tests or JavaScript parsers ran. This
JavaScript-only change required no repeat Rust compilation.

Known ceiling: Multi-host competitive HUD and coordinated room final drain
remain required. Room stop currently cancels and joins the channel rather than
proving whole-room completion, even when local receipt flags are complete.
JavaScript execution, generated bindings, runtime TLS/browser, devices/audio/
performance and formal review/QA remain unverified; the full player Goal stays
active.
