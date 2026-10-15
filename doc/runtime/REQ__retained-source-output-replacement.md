# Retained Source-coordinate output replacement

## ASIO complete Source path — implementation contract

The ASIO vertical under development follows the same common Source Owner,
typed Output UI and solo/cohort pumps. Preserve NativeOutputState, original
pending PCM/report/admitted prefix, Source history and creation identity through
same-family replacement. The flow pauses playback, stops and reconfigures native
output, then resumes the same play. A silent reconfiguration gap is expected;
uninterrupted sound or physically pop-free switching is not established by
software PCM preservation. This section defines intended behavior; implementation
and available proof are tracked in WBS07.08.07.14 and the
[ASIO change record](../changes/2026-10-15-asio-retained-source-output.md).

Store the actual ordered native channel indices and heterogeneous PCM encodings
in a checked planar layout; never claim an interleaved Float32 native format.
Pending PCM permits only the exact same layout and remix matrix. Cold storage
uses actual native frames within the immutable Mixer rendering ceiling.
Retain the original whole RenderReport beside the positively delivered first
frame/count. Coordinates, history validation and finite-end acknowledgements
use that delivered interval. Whole-report-only scalar pause/replay consumers
must reject partial delivery explicitly, including a prefix at report start.

The sole callback retirement decision precedes native clock acquisition,
parsing, raw publication and Source work. Authorized retirement zeroes each
selected actual double-buffer half on every callback without Source admission.
A Continue permit is consumed once; a later request waits for the next callback.
Validate the whole prefix and every plane before copying; an uncertain
post-copy admission quarantines the owner rather than replaying possibly heard PCM.
Ready complete preparation is unprimed; prime B immediately before Start and
let a synchronous A callback consume the next prefix. Failed attempted Start
quarantines possibly presented B even with zero recorded callbacks.

Retain the first two progressing original ASIO intervals until HOST covers
their original upper endpoints. Validate live/errors before cached release;
never substitute receipt time, midpoint-only evidence or a fabricated frame.
Keep existing timeout, cancellation, age, frozen-history and atomic epoch gates.
Callback retirement and successful driver release are separate proofs. Preserve
typed primary/cleanup errors, actual pending state and hidden HWND custody on
failure; a consumed failed native handle cannot promise a cleanup retry. An
unproved release stays unproved across repeated stop and Drop, retaining the
HWND conservatively if necessary. Authentic SDK/MSVC and installed-driver
acceptance remain required, separately from portable tests and foreign typing.

## CoreAudio complete Source path

CoreAudio retained solo/local play uses the same common Source Owner, typed UI,
conditional mapping/control/history and paused-coordinate publication ports.
Native adapters own HAL effects; common gameplay policy, Runtime/RuntimeGroup,
attempts, selected policy and capture remain shared and unchanged. Network and
legacy scalar/remixed consumers keep their existing compatible path.

Preserve complete NativeOutputState through native open/start/retirement/cleanup
and recover it only after Stop/unregister/drain. Reopen at the first unsent frame
and retain the original Source history identity and creation pin. Actual original
Mach/sample/flags and coherent even-revision native delivery packets qualify
publication; a created SourceFrame descriptor alone grants no progress.

An original native presentation timestamp may lie ahead of the current HOST
poll. During replacement, retain qualifying typed SourceFrame observations and
wait for their original HOST endpoints; do not fail, retimestamp or continuously
replace the selected anchors with newer future callbacks. Two genuinely
progressing original anchors, current live/error checks, bounded timeout,
cancellation, stale-age and atomic publication checks still apply. Cache
expectations come from the actual output epoch/basis, requested Source identity
and backend HOST domain; malformed first or later packets refuse immediately.
Evidence ownership moves without requiring Clone/Copy. Waiting grants no native
admission, drain or new clock authority.

A history admission request can race the sole callback's retirement approval.
The CoreAudio adapter classifies only Native(RecoveryUnavailable) for the
existing Owner re-probe; defer only confirmed authorization. Without that
confirmation, preserve the error. Configuration, callback, packet, identity,
timing and unrelated native errors remain visible even while future evidence
is retained.

Support actual variable callback sizes within the cold advertised maximum,
separate from nominal requested frames. With pending PCM, submit only a full
callback prefix that already exists, retaining the original report; never pad or
rerender a short suffix. Cold fixed-buffer reopening accepts an empty suffix or
a whole multiple of the new buffer. Live same-interpretation retirement permits
empty or proven paused-zero suffix with compatible fixed geometry; changed
interpretation or variable destination requires empty suffix. Incompatible
geometry refuses intact, with no silent device/mode fallback.

Retained composition uses a bounded immutable Mixer growth ceiling and prepares
PCM/history from actual cold maximum, preserving source history across capacity
growth. Every group is validated before any native copy. Retirement precedes
source work and publication; HAL's pre-zeroed silence after authorization never
advances source admission. Native authorization and actual callback drain remain
distinct. See [complete CoreAudio recovery](../kernel/REQ__coreaudio-recoverable-open.md).

The actual adapter/UI/solo/local wiring, portable production callback execution,
foreign Rust typing and real Mac/HAL/device acceptance are distinct evidence
tiers. CoreAudio WBS07.08.07.15 remains unfinished until its required real native
acceptance; absence of a Mac host does not waive the full player requirement.

This is an implementation contract, not a completion claim. It extends the
[gapless contract](../kernel/REQ__gapless-practice.md). The historical task slug
contains sourceframe; actual WASAPI provides SourceCounter. Preserve both true
Source coordinate classes, with WASAPI shared Counter consumption implemented
first and same-family exclusive consumption following the same complete owner.
Other backends/formats and physical/browser acceptance
remain full player requirements.

## Same play and complete ownership

After genuine pause adoption and native/input cutoff, supported same-family WASAPI
endpoint, buffer, period, policy and matrix/channel changes resume the same
Runtime, Mixer/program/PCM/command producer, Source history, selected policy,
attempt/section/generation, accepted input and capture. No new attempt or silent
fallback. This vertical preserves original Source rate and existing encoding;
remaining rate/encoding controls and resampling stay required follow-ons.

The actual adopted startup gate may follow stream creation. Preserve its distinct
playback origin at the first replacement: require the exact original Source point
at max(adopted startup frame, current binding's first physical frame). Later
epochs advance that binding; validate the resulting exact point rather than an
arbitrary later timestamp. Keep stream origin and all cold/final pause identity
checks intact. A future gate does not justify fabricating an earlier native seed.

Select an explicit complete owner for retained WASAPI shared and exclusive startup.
Shared EnginePeriod/DeviceDefault policy changes remain supported within that
family; exclusive replacements retain exclusive mode. Cross-family and backend
changes refuse cold. ASIO keeps its existing Legacy startup path and explicit
retained-replacement refusal until its own supported vertical is implemented.
Never change the selected mode or backend to obtain a supported path. Preserve
legacy Mixer/remixed/scalar/finite/ASIO recovery and scalar backend switching;
do not route legacy remixed recovery through NativeOutputState.into_mixer,
which intentionally refuses a prepared remix. Use static application variants
and portable gameplay policy, not duplicated OS pause/replace logic.

Exclusive event-driven fills acquire, encode, release and positively admit exactly
the actual applied native buffer size, without a padding query. Both Ready prefill
and running event work use this rule; timeout/control-wake clock-only work retains
the same conditional permission gate. Exclusive timer mode remains explicitly
unsupported. No requested/default buffer value is fabricated as native evidence.

Exclusive live retirement always requires an actually empty suffix at the sole
worker decision. A recovered complete owner may nevertheless contain original PCM
from a prior setup/local failure or a public recovered-state caller. Preserve and
support an empty suffix or a suffix that is a whole multiple of the new actual
native buffer: eight retained frames can be submitted as two four-frame leases.
Never re-render, discard or pad those bytes. A smaller or nonmultiple suffix
refuses intact before any history/native lease/clock work. Existing format/matrix
compatibility and uncertain-admission quarantine still apply.

Before native acquisition, reject incompatible pending PCM only when validated
Exact Frames/Duration/explicit Period determine the buffer size. Reuse existing
duration/period arithmetic; unresolved defaults and caller-authorized rounding
remain unknown. After actual GetBufferSize and native buffer/period equality,
check the same whole-multiple rule before history preparation, reconfiguration,
MMCSS registration or prefill. A refusal preserves the complete software owner
while initialized native resources are retired through existing cleanup paths.
Use the immutable Mixer render limit rather than the old prepared max_frames to
allow legitimate native buffer growth. Geometry errors remain typed local
failures and cannot be hidden by the native IO authorization classifier.

Retained NativeOutputState owns Mixer, remix, generated PCM/report, admitted
prefix and history through open/start/stop/join/recovery. Never flatten it to
Mixer. New native basis is its first unsent frame, not the pulled Mixer frontier.
Keep the original immutable creation pin separately from that advanced basis.
Prepare initial Source history from the actual native buffer geometry before
Ready prefill through a controlled cold method on that owner. Preserve existing
history and pending PCM; do not expose unrestricted mutable Mixer access.

## Exact native evidence and publication

WASAPI retains actual IAudioClock position/frequency; never relabel Counter as
Frame or copy old frequency/PCM Hz. New epochs may have different actual Counter
frequency, but descriptor and observations must agree. Created metadata and
Ready prefill grant no native progress. Source Hz/history identity stay original.

Reuse exact AudioAuthority coordinate preparation and semantic merger binding.
Require same class, newer epoch, unchanged physical origin/HOST/logical domains,
checked prior logical projection and all exact/chronology frontiers. No scalar
permissions. Two fresh progressing original native coordinates and coherent
Source packets qualify publication, even when rounded nanoseconds are equal.
Use the matching genuine WASAPI/ASIO/frame validator.

Opaque Source preparation binds prior native record/frame position, authority,
descriptor, pin and merger. Prove the same unique recovered owner/history before
relaxing Source pin equality for a new advanced basis. Validate actual capability
before borrowed atomic authority/native/pause/current commit; refusal retains
the intact Ready output and exclusive hold. Stale state rejects without mutation.

Capture a cold Source pause proof before moving state into open; complete it
with actual created descriptor and fresh native packets. Preserve acknowledged
Paused/frozen playback, source rate/origin/initial identity, exact gaps, adopted
startup frame, pull monotonicity and HOST floor. Clear old native brackets and
invalidate prior control/practice tokens. Resume requires later actual adoption
and crossing. A merged Frozen span may begin before the new basis; preserve it
and require fresh paused render/submission/native progress rather than resetting
history. Enforce zero playback advancement, correct frozen cursor, even revision
and submitted<=pulled provenance.

## Sole-worker decision, admission and failure

Validate immutable settings/capacity/matrix before live retirement. The sole
request sizing check must reject oversized positive durations and mathematically
nonintegral Exact PCM frame durations before a ticket or native effect. Preserve
explicitly authorized rounding and leave unknown native defaults/bounds unknown.
The retained UI validates its actual resolved request before Owner scheduling;
invalid drafts receive one correlated refusal and preserve the live owner,
capability and epoch. Genuine native IO failures keep their original semantics.
The sole
worker evaluates actual pending state before history/render/native acquire,
encode/release or clock-only timeout work. Same interpretation permits only
empty or proven paused-zero suffix; changed interpretation requires empty PCM.
Refusal preserves live owner/epoch/capability. An observed-empty snapshot or
cached Pending status cannot authorize termination or prove liveness.

Tickets weakly identify original control allocation with checked nonces. Busy
and unconsumed outcomes cannot be overwritten. Cancellation and authorization
share one atomic decision. Permission forbids later effects but does not prove
Stop/join/recovery complete. Finish an already acquired synchronous lease before
the next decision; every acquire has exactly one release, silent on local error.

Only successful positive release commits admission before later telemetry.
Failed release leaves admission uncertain: preserve complete owner for diagnosis
and cleanup, quarantined against ordinary reconfigure/open/publication and
explicit Retry. Never replay uncertain PCM. No resync policy is invented;
technical stop is visible. Preserve original phase/code plus release, Stop,
start cleanup, join and recovery failures, not a single overwritten HRESULT.
Complete cold setup also records MMCSS teardown failure on every exit after
registration, including priority, preparation, prefill and pre-run publication
failure. Keep the original operation failure and complete recovered owner; a
Result-discarding resource destructor cannot provide this diagnostic guarantee.

Source observation/query/history requests probe before old IO and re-probe only
raw backend failures, deferring only confirmed authorization. Genuine backend,
identity/policy/timing and complete probe/cleanup/recovery errors remain visible.
An adapter's backend error envelope alone does not classify an error as native
IO. A static, default-false backend classifier identifies only raw IO/liveness;
typed packet, coordinate, history-range and timing failures remain visible even
when authorization follows. Remix forwards the same classification unchanged.
History validates original range before clamping to pending report start and
practice pins; a clamped request remains pending. Keep existing Mixer helpers
compatible and avoid unrestricted mutable Mixer access.

## UI, pumps and verification

Use typed Source service and actual Windows solo/local coordinate hook. Common
solo/cohort mapping queries use an additive mutable device port so the Source
Owner can probe conditional permission and retain recovery errors during the
query itself. Its default forwards the existing read-only mapping API unchanged;
the complete Windows Source path overrides it with the conditional Owner query.
No runtime borrow cell is needed to service this IO from the common pumps.

WASAPI request planning distinguishes an omitted device field from an explicit
blank one. Omission keeps the selected endpoint; explicit blank requests the
actual current OS default multimedia endpoint, discovered by the native adapter.
An explicit nonempty endpoint remains exact. Do not silently retain the previous
endpoint after an explicit default-device request or invent a default identity.

Common pumps own acquired-prefix paused input, suspended polling, practice/replacement
fences and actual resume. Preserve request ID and one correlated reply across
WouldBlock; no duplicate transaction. Non-None coordinate NativeEnd and pending
practice refuse cold; retained completion uses PracticePlayback.end_intent.

Tests cover both coordinate classes, exact frequency changes, same-owner pin,
fresh native proof, stale-state atomic rejection and real pause/resume. Portable
WASAPI tests execute the exact production worker step, slot/fill/clock decisions,
lease/error combinations, history/pending preservation and uncertain retry
refusal. Calibrate allocation outside libraries where unsafe code is forbidden.

Connected actual common pumps/Owner/UI/Runtime and distinct-member RuntimeGroup
cover nonzero PCM, partial tails, change/refusal/publication/resume/repeated
change/loop/fractional scrub/end, paused Down/Up and original capture facts,
metadata and final live hashes. Preserve Source/scalar/finite/Target compatibility.
Portable planner/worker/common-pump, Windows-gated composition, foreign typing
and actual Windows execution are distinct claims.

Actual review precedes CLI QA and highest available native GUI verification.
Eligible Windows clock/input/audio is required for physical acceptance; Linux,
stub compilers, ALSA null and screenshots cannot supply it. Preserve bounded
hardware/OS blockers while continuing other software Goal work. Track Source
milestones and every remaining backend/mode in WBS; no whole-parent completion
from this first vertical.
