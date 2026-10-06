# Preserve acknowledged pause across a replaced output stream

NativePause exposes an output epoch, initially zero, and a cold rebind_output
operation accepting a strictly newer epoch and the actual recovered Mixer.
Require an acknowledged Paused phase, paused mixer, identical original frame
grid domain/origin/rate, exact frozen playback cursor, nonregressing physical
cursor and compatible immutable startup/finite settings. Reject an already
reached finite endpoint. Checked epoch/frame/timestamp bounds refuse atomically;
max-u64 never wraps. The caller must have retired the old native owner before
using its unique recovered mixer; this method performs no retirement or IO.
Read-only Mixer startup getters expose the actual queue gate state (ungated,
gated/unarmed or armed frame) and immutable applied first-playback evidence.
Use existing release/acquire publication; no new synchronization path. Gated
rebind requires armed and applied startup frames to match the pause prefix.
Unresolved startup evidence is refused rather than inferred from cursor gaps.

Success preserves frozen playback, cumulative acknowledged pause gap, startup
and finite endpoint configuration. Do not replace that gap with the mixer's
latest physical-minus-playback difference: the eventual observed resume commits
the cumulative gap using the existing logic. Clear previous stream interpolation,
point/interval kind and report history, allowing new point or interval evidence.
Keep setup locked. Establish a physical lower bound at the captured mixer next
frame and preserve the greatest actual observed host frontier from accepted
point pairs/interval arrival times. Fresh reports and point relations cannot
precede those frontiers; interval arrival times remain monotonic. Uncertainty
endpoints are not substituted for actual observed arrival times.

Tagged point/interval request and observe APIs reject mismatched epochs before
interpreting metadata, changing evidence kind, freshness or state. Existing
untagged APIs assume the current owner and remain compatible; they cannot by
themselves identify late old-stream messages. Assign epoch at stream creation,
not at delayed observation admission. Rebinding supplies no fake first sample,
resume boundary or acoustic synchronization proof.

Author independent real paused-mixer cases for point/interval replacement and
continued genuine resume, nonzero/max epochs, stale tokens, frame/host floors,
atomic phase/grid/cursor/endpoint refusal and repeated replacement. Preserve
original PCM/commands, finite endpoint behavior and cumulative rounding rules.
No new crate, heap allocation, native import or per-note work. The fixed pause
owner gets only scalar stream guards. Full runtime/backend/UI handoff, sample
rate/grid conversion and device/acoustic acceptance remain subsequent work.
Assertions/runtime/formal review/QA remain deferred; scoped Rustfmt and four
sequential compile-only checks follow both paired writer terminal stops.
Joint pause/presentation staging and translation to the fresh stream-zero anchor
now follow [staged output timing](REQ__staged-output-timing.md). Keep the producer
pause request pinned through native Ready priming; applied pause alone does not
prevent a queued resume request from advancing the mixer during opening.
