# Shared room progress publication cadence

Native RoomCompetition and the actual browser room publication path use common
Rust cadence policy. Reuse ProgressCadence and its existing 50,000,000 ns interval;
avoid another timer policy or browser/native duplicate interval calculation.
First ordinary publication is due immediately; later ordinary publication is due
at the exact interval boundary. Final prefixes bypass interval suppression but
never chronology, phase, original roster, counter or one-shot final checks.

Cadence observations use caller-supplied nonnegative integer time and reject
regression, including a regression after a suppressed observation. Successful
queue admission commits the publication marker. Refusal/backpressure retains
that marker. This is admission cadence, not an actual transport write or ACK.
Existing generic progress publication retains its post-effect clock semantics.
No rejected/suppressed call may invent receipts or consume upload sequence IDs.

RoomClientDriver adds a scalar publication-due query and a cadence-aware bounded
words publication method, keeping the old publication API compatible. The query
is a timing hint, never permission to start/play/complete. Actual publication
performs complete shared room phase, exact roster and progress preflight before
suppression/commit. Extract existing validation if necessary without duplicating
the algorithm or allocating another serialized message. Invalid time/evidence
retains existing fatal classification; recoverable local state refusal remains
recoverable. Final admission cannot be repeated.

BrowserRoomClient exposes exact i64/BigInt observations and bool admission results.
BrowserRoomOwner supplies its actual elapsed clock, delegates due/admission to
Rust and wakes the writer only after true admission. Worker checks the scalar due
hint before constructing progress_words and publishes through the same owner.
Remove room.lastProgress and its JavaScript interval calculation; non-room
multiplayer cadence is outside this increment. Preserve idle/start/Leave/closed
guards and finalQueued only after an accepted final. Native room controller
retains every valid local prefix while suppressing only network publication.
An unacknowledged Progress command prevents another ordinary publication before
clock access. It does not close the room or disable its Connected UI status.
Only the original Leave/terminal/cancel/finish conditions determine closure;
local progress validation and retention continue while the command is pending.

Pure clocks and the existing split write-completion driver remain separate:
async promises are not std::io write completion. No new IO/timer/thread/lock,
dynamic per-note dispatch, crate or Window rendering is required. Full browser
RoomCompetition/RoomNetworkActor composition remains subsequent work.

Author common boundary/chronology/admission rollback, actual driver protocol and
native controller fixtures plus actual Owner/Worker delegation cases. Existing
mock bindings gain the new methods without weakening assertions. The user lifted
verification deferral on 2026-10-06; execute applicable tests and checks alongside
implementation. Final independent review/QA still gates task close. Browser and
native execution count only when actually performed.
Measured performance, real WebTransport and hardware acceptance remain unproven;
full BMS player Goal stays active.
