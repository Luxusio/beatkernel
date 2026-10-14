# Retained Source-coordinate output replacement

This is an implementation contract, not a completion claim. It extends the
[gapless contract](../kernel/REQ__gapless-practice.md). The historical task slug
contains sourceframe; actual WASAPI provides SourceCounter. Preserve both true
Source coordinate classes, with supported WASAPI shared Counter consumption
implemented first. Other modes/backends/formats and physical/browser acceptance
remain full player requirements.

## Same play and complete ownership

After genuine pause adoption and native/input cutoff, supported existing shared
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

Select an explicit complete owner only for retained WASAPI shared startup.
Retained exclusive and ASIO keep their existing Legacy startup path and explicit
retained-replacement refusal until their own supported vertical is implemented.
Never change the selected mode or backend to obtain this shared-only path. Preserve
legacy Mixer/remixed/scalar/finite/ASIO recovery and scalar backend switching;
do not route legacy remixed recovery through NativeOutputState.into_mixer,
which intentionally refuses a prepared remix. Use static application variants
and portable gameplay policy, not duplicated OS pause/replace logic.

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
No runtime borrow cell is needed to service this IO from the common pumps. Common
pumps own acquired-prefix paused input, suspended polling, practice/replacement
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
