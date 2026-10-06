# Stage compatible pause and presentation clocks for replacement output

GameplayPresentationPort stages a fresh output observer at a strictly newer
explicit epoch, retaining actual discipline configuration. Refuse unsupported
epochs and mismatched pause/presentation epochs before constructing a candidate.
PreparedOutputTiming contains the candidate NativePause, presentation owner,
captured OutputFrameBasis and new playback origin; it supplies no stream or
accepted observation. Both old clock owners and the mixer remain unchanged on
any preparation refusal. Use static ports and caller-owned epoch identity.

Require actual acknowledged paused mixer state and an active producer pause
request. Expose that desired state through a read-only Mixer getter. The caller
must keep pause requested throughout native opening/Ready priming, since WASAPI
and ASIO priming render the actual mixer. Do not silently resume while preparing.
Create candidate pause through its existing rebind validation and derive the
new output/playback origin from captured stream frame zero. Preserve the
original logical frame grid and pause host domain; no grid conversion or fake
clock sample. Consume unique epochs when actual stream attempts are created,
including failed attempts, rather than reusing tags from discarded candidates.

NativePause computes song_origin_for_presentation(original, playback_origin)
as original minus the cumulative manual-gap nanoseconds plus the difference
between the supplied presentation origin and original startup point. Combine
using wide integers before final timestamp narrowing, preserving existing
integer mapping and fractional rounding. Reject wrong domain or origin before
startup. The original startup origin yields the existing after-pause value.
This offsets the new stream-zero anchor without changing logical song origin,
frozen playback, transport history or judged/captured input.

Actual solo/cohort resume paths use this calculation with their configured
presentation origin, retaining original native seeding and evidence-before-clock
publication. Staged new observers have empty history and require genuine
epoch-tagged samples/warmup. No Mixer cloning, per-note work, native import or
callback allocation. Fresh presentation storage allocation remains cold.

Author independent actual Mixer/Core/native presentation-port staging cases,
epoch/config/refusal atomicity and old/new mapping equivalence, including startup
offsets, noninteger rates, wide overflow cancellation and subsequent genuine
resume. Assertions/runtime/device/formal review/QA remain deferred; scoped
Rustfmt and four sequential compile-only checks follow both writer terminal
stops. Actual backend/UI handoff, error fallback and physical acceptance remain
subsequent work; full BMS player Goal remains active.
