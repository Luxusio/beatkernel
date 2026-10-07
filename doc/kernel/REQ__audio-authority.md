# Shared audio-output authority and acquired-input correlation

Status: selected contract; implementation is in progress. This common business
boundary implements [audio-authoritative playback](ADR__audio-authoritative-playback.md).
It reads no native clock or device and performs no rendering or Transport rate
correction. Existing `InputMerger` owns event ordering, source sequences,
pending storage and byte limits; do not duplicate that queue.

## Clock and progress ownership

Keep three distinct domains: original HOST acquisition time, raw stream output
and scheduling time, and the normalized logical audio timeline. A replacement
stream may restart at frame zero while logical playback continues, so domain
identity must not silently equate different origins. The active epoch supplies
raw/logical origins and the HOST domain. Rebase actual raw output observations
with checked integer arithmetic; preserve the logical domain across epochs.

Retain a cold-preallocated finite history of real paired observations. Two
strictly increasing pairs are required for input correlation. Use the core
unknown-quality affine constructor; no guessed error bound or exact unit-rate
relation. Configurable finite backward extrapolation can cover startup input
before the first observation. Finite forward extrapolation also respects an
explicit maximum logical-output prediction distance. These are permissions,
not physical accuracy guarantees; default policy forbids extrapolation.

Acquired HOST prefix, processed/closed HOST prefix, committed input occurrence,
committed logical operation and actual committed presentation are separate
watermarks. Recording an acquired prefix alone processes no input and closes
no deadline. A presentation point is eligible only when its original HOST
association lies within the fully acquired prefix.
Select the most recent fresh retained real observation covered by that prefix.
The newest observation need not be covered: requiring it exclusively would
starve acquisition that intentionally keeps a lag behind every newest sample.
An older covered observation authorizes only its own actual output position.

Before advancing deadlines, process every input through that entire prefix,
including equal-time inputs. Then close the existing merger at the full prefix,
not merely at the observation's HOST association. Preserve original acquisition
metadata through Runtime normalization. Correlation is fixed when an operation
commits and newer observations cannot reinterpret it.

If permitted input prediction commits an operation ahead of actual presentation,
retain both positions. Later observed output below or equal to the operation
point may update presentation but must not call Runtime backward. Advance only
when actual output has caught up and can progress monotonically. Prediction
never grants actual presentation or natural-completion evidence. Generated
render cursors remain separate command-scheduling/drain evidence.

## Bounds and staged effects

Use 2..1024 retained observation slots, positive finite observation age and
nonnegative finite prediction limits. Repeated, absent or stale output cannot
gain progress from host timers. Wrong domain, epoch, regressing observations,
arithmetic overflow, expired required history or full pinned storage refuse
explicitly before mutating admitted state.
Freshness applies to the selected covered observation. A newer association
whose original HOST target is still future does not invalidate older retained
evidence that is due and fresh. The selected age at the declared query time must
be nonnegative and within the configured limit. An input occurrence or acquired prefix beyond
that declared current HOST time cannot authorize an operation yet; hold without
changing the queue, mapping history or watermarks.

Retire history only through the fully closed original HOST prefix while keeping
the most recent two real anchors. Never evict a segment needed by pending input.
`InputMerger::peek_ready` validates a frontier and exposes the earliest eligible
event without popping it or changing byte/sequence accounting. Prepare its
mapping before removal and dispatch it once through the real Runtime.

Preparation descriptors are immutable and record the exact state relevant to
their operation. Commit rejects stale revision, wrong epoch and mismatched
configuration, watermarks, guards or selected mapping/result. Re-prepare that
operation before committing, including its actual mapping or frontier when it
depends on retained history. Operation-equivalent owners may accept: clearing
operations need not distinguish unused interior observations that they discard.
An interior change that alters the selected mapping/result must refuse. No
global counter, hash, clock read, full-history token copy or allocation is needed.
Rejections preserve every prior watermark and retained observation.

Output replacement requires a strictly newer epoch, unchanged HOST and logical domains,
logical origin at or beyond the committed operation and zero pending input.
Validate and prepare before committing; recheck pending input and staged state.
Retired observations cannot regain authority.
Retained acquisition watermarks belong to the same original HOST domain and
merger. A different HOST clock requires a new session and merger; output
replacement must not retimestamp that provenance.

Ordinary same-stream resume retains the physical epoch, correlation history,
configuration and all acquired/closed/committed watermarks. An explicitly
requested correlation restart is a separate operation: clear only observation
history using reserved storage and require two fresh pairs. Consumer lifecycle
owns Transport pause/resume; neither resume nor restart resets judged history.

## Verification cues

AudioWorklet terminal diagnostics must retain the first failure's original
status, operation origin, processor phase, actual callback frame and block
extent when known, and native retained expected/start frame words with explicit
presence. The successful arm's exact context frame may be retained as operation
evidence. These fields never authorize output, judgment, capture completion or
clock correction. Preserve unknown values as absence rather than inferred zero.
Preallocate the payload; successful process callbacks do not poll diagnostic
reports, allocate views/objects or convert BigInt values. First-failure scalar
reads preserve the original status even if a getter fails. Host, command and
sample owners validate and retain the bounded snapshot through errors and
cleanup; legacy status-only terminals remain compatible. Later failures and
retirement cannot overwrite the original cause. Verify exact high/low words,
zero versus absence, malformed/stale terminals and all owned delivery ports.
Rejected control ACKs retain their existing order and admitted-prefix fields.
Capture first-cause facts before posting the ACK and include the same bounded
snapshot as optional diagnostics, so a client that retires on the rejection
still retains the evidence. A later terminal must not replace that error.

Use pure memory fixtures with actual `InputMerger`: two-anchor startup, analytic
unequal-rate mapping, original input provenance, backward/forward permission
limits, prefix lag, equality ordering, predicted-input catch-up, stationary and
stale output, retained delayed input, pinned capacity and retirement, domain and
epoch faults, failed replacement, same-epoch restart, stale and cross-owner
descriptors. Assert complete state preservation on rejection.

These fixtures establish the boundary only. Shared Step/native/browser owners,
native startup and lifecycle now select the authority; capture/replay has a
joined native roundtrip fixture. The earlier browser playback refusal was
corrected and passed three independent pre-review Chromium smoke executions.
Fresh full regression, replay compatibility and independent CLI, desktop and
actual current-browser QA remain required; development diagnostics do not
establish final acceptance.

## Shared Step gameplay integration

Audio-authoritative solo and local construction consumes one authority and
retains the adapter's existing merger separately. One cohort uses one shared
authority and transport. Construct the logical transport on the epoch's logical
origin with the existing section/preroll song position and normal rate; keep
the raw audio scheduling domain and original HOST acquisition domain separate.
Capture and competition setup use the logical normalization domain.

Before popping an input, validate mode, activation, scheduling domain and any
spatial position, then privately prepare its mapping in the current owner.
Unavailable evidence retains the original event. Dispatch once with the
prepared mapper and actual raw scheduling point. Commit accepted timing before
score, gauge, mine or capture postprocessing: a later observer error cannot undo
the real Runtime report. Pre-report failures fence without a fictional timing
commit. Partial local reports retain the actual processed prefix and commit its
point once; never retry earlier members. Explicit ignored-source input does not
invent a logical operation.

Privately prepare a frontier. Dispatch only its monotonic logical advance;
presentation-only closure updates the merger and observed watermark without a
backward Runtime call. Prepare revision changes before effect permission so
overflow cannot first appear after Runtime side effects.

Audio activation preserves the logical anchor. Old generic HOST activation,
rate-correction configuration/update, generic input and generic deadline methods
must reject an audio-authoritative instance before mutation. Explicit generic
legacy constructors remain available; BMS production selects the audio path.
Verify genuine Runtime/RuntimeGroup, captures and command queues, including
three-domain provenance, delayed input, stationary output, partial/post-report
errors and unchanged state on forbidden generic operations.

Output-evidence admission is separate from completion evaluation. Validate each
report and presented point with the existing stop-aware validator, then retain
the latest nonempty render report and normalized presentation for chronology.
Admission must not evaluate readiness, create completed results or update drain
state. Local owners delegate to their shared control. Validate before BGM credit
or replacing the binding's evidence DTO, even while queued inputs prevent
completion. A newer context cursor cannot hide regressing render counters or
presentation; verify rejection before any new input or BGM effects.

## Primed replacement and resume correlation

Stage two strictly progressing original raw-output/HOST associations inline
before publishing a replacement epoch or restarting correlation in the same
epoch. Preparation changes no active state and allocates no new history.
Publication revalidates the staging token, domains, checked origins/arithmetic,
current freshness and absence of pending acquired input. Reject repeated output,
future or stale evidence and regressing committed presentation/HOST history.
Evidence may precede the current acquired prefix; acquisition itself must not
force an invented observation. A same-epoch restart preserves the existing
distinction between actual presentation and a permitted predicted operation.

Install the two associations in already reserved history while preserving all
acquired, closed, original-input, operation and presentation watermarks. Priming
alone neither consumes input nor advances gameplay, song or presentation.
Refusal retains the previous active epoch and history. Lifecycle consumers must
validate immediately before committing and make only infallible ownership swaps
after commit; release the held output last. The existing clear-and-wait APIs
remain available for explicitly selected consumers. Pure staging fixtures do
not establish native replacement or resume acceptance.

Ordinary pause/resume on the same output epoch and frame basis retains continuous
raw physical-output/HOST correlation: physical counters progress while playback
frames are frozen. Do not reset correlation merely because playback resumes.
Explicit correlation restart or new-epoch publication occurs while output is
acknowledged paused and held, after paused inputs have drained through the
existing keyboard reconciliation path. Do not request unpause and then attempt
to prime while post-resume inputs are already pending.

The native pause transition must retain its original physical output boundary
alongside the separate conservative HOST acquisition cutoff. Rebase that native
boundary into the logical audio domain for control operations; a HOST midpoint
cannot replace it. Control-operation commitment must not claim a physical input
occurrence. Servicing paused presentation/prefixes must not invent a Runtime
operation. Shared native audio loops and held-output publication implement these
lifecycle rules; production launchers select them. Physical device execution and
independent lifecycle QA remain pending.

The additive native pause update carries the original HOST window/cutoff, output
epoch and raw output point from the committed physical transition frame.
Retain that physical marker privately after both point and interval admission;
do not reconstruct it from playback frames or song time. A waiting or unchanged
observation emits no transition, and rejected updates retain the prior marker
and all pause state. Successful output rebind clears retired transition evidence;
failed rebind preserves it. The audio update stages conversion with the existing
pause validation before changing the caller's owner. Existing HOST pause APIs
retain their behavior until their consumers explicitly select audio authority.
Stage audio Transport pause/seek/resume at the prepared logical control point.
Require matching boundary epoch, raw output point and original HOST cutoff;
retain the exact frozen-song guards. Preparation returns a candidate without
mutating active Transport, pause or authority. Installing the candidate and
committing a genuine Runtime report remain the lifecycle consumer's transaction.

Prepare native control operations from the validated raw physical boundary and
separate original HOST cutoff. Require current epoch/domains, checked rebasing,
nonregressing actual operation and accepted input chronology, acquired HOST
coverage, two fresh real anchors, observed raw coverage and no pending input at
or before the cutoff. Preparation grants no operation; commit follows a genuine
Runtime control report and changes only the logical operation and revision.
Preserve original input, acquisition, closure and presentation history. A cutoff
may precede an already closed acquired prefix or held presentation; neither
implies an already committed gameplay operation. This seam trusts the validated
native boundary provider and makes no physical accuracy assertion.

While the producer is acknowledged held, an explicit observation-only frontier
may close the complete acquired prefix and retain actual presentation without
changing the committed Runtime operation or original input occurrence. Validate
the same freshness, ready-input and semantic-token rules before commit. This
permits finite history retirement during a long pause without fabricated reports
or song progression. The ordinary operational frontier retains its existing
behavior; callers must select held servicing only in the held lifecycle state.

The shared native gameplay loops select timing through static wrappers. Legacy
entry points retain their HOST/correction and FIFO behavior; explicitly selected
audio entry points own native validation/authority and a retained input merger,
with no instantiated legacy estimator. Both paths share judge, gauge, recording,
competition, pause, completion and stop handling. Audio input mapping precedes
consumption, and actual reports commit authority before fallible observers.
Partial group reports commit once; a pre-report failure commits nothing.

Native audio clock unavailability must fail explicitly after its configured
bounded interval, rather than remain indefinitely playing. HOST acquisition and
closed-prefix gates remain separate from logical Runtime operations. Actual
native end evidence, complete original input closure, logical judge completion
and stop/drain evidence all remain necessary for finite completion. The shared
loop migration and production constructor/startup selection are connected in
Linux, Windows and macOS solo/local launchers. Portable fixtures establish
development behavior; platform checks and actual native QA remain separate.

An original native association may describe output whose assessed presentation
HOST time is still future, as with explicit ASIO driver latency. Retain it
unchanged as calibration evidence. Freshness may come from any retained actual
association already due within the configured age; a newer future association
must not block an older fresh covered one. Frontier selection still requires
the chosen association to be covered by the acquired prefix and fresh at the
current sample, and the prefix itself cannot be future. All-future or stale-past
history grants no operation. Preserve event-future, validity and input-ahead
checks; do not clamp timestamps, remove driver latency or treat future evidence
as current presentation.

Cold native audio admission reads actual Runtime normalized and audio scheduling
domains before device or host effects, even when capture is disabled. Every
cohort member must agree with the authority's logical timeline and raw output
domain. A shared runtime getter verifies actual members rather than cached
construction metadata. Equal origin timestamps, headers or a successful probe
report cannot establish this identity; refusal must not dispatch input or alter
Runtime. Generic legacy consumers retain their existing domain interpretation.

Pause control preparation requires no pending input at or before its HOST
cutoff. Resume preparation blocks only strictly earlier input; an original event
at the resume cutoff remains in the merger and dispatches after reconciliation
and the actual control operation. Bind this purpose into the descriptor and
check it when staging Transport. Do not pop equality into another queue or
classify it as paused. Its original HOST metadata is recorded only when the
actual input report commits.

Production startup constructs one cold native authority/validator from the actual
stream frame basis and explicit distinct HOST/raw/logical domains. Retain two
original progressing associations; while a future second association is waiting
for coverage, keep those two fixed and continue actual render credits, terminal
status checks and retained input service. A positive bounded timeout, host
domain/chronology and association freshness remain required. Cancellation
returns without inventing startup permission. The input-service boolean denotes
continuation only and never certifies a drained prefix.

Offline software acquisition origin may use the two original associations with
unknown accuracy and explicit finite backward projection; do not substitute a
nominal rate or claim an exact acoustic start. Network startup retains its
existing observed frame plan and conservative HOST window. Transport anchors at
the logical coordinate of the actual selected raw playback origin. Keep retained
startup inputs until the real input owner drains them before prefix closure.
Cold Runtime anchor validation uses the logical coordinate of the actual
selected playback frame, which can differ from the epoch's raw stream-zero
coordinate. Preserve stream zero and require checked domains, exact song anchor,
normal rate and pristine state. Backward projection permission also extends its
explicit raw validity interval; it never grants unbounded or forward prediction.

Audio gameplay configuration carries the original requested section start
separately from its clock/pause configuration. Preroll changes the initial song
anchor, and gated startup changes the physical playback frame; neither changes
the requested chart section. Validate capture and competition setup against this
explicit start, with unchanged exact chart/profile/gauge/class/clock identity.
Require a nonnegative section start, a valid initial song anchor at or before it
and an endpoint at or after it. Do not derive section identity from the physical
frame, copy it from an unvalidated header or change stored record interpretation.
Legacy generic configuration APIs retain their existing behavior.

Finite offline startup preserves its first original native lower observation.
If that observation establishes an actual zero/short endpoint boundary, retain
the boundary for normal gameplay observation to deliver once; priming must not
consume and discard completion. Invalid later observation retains deferred
delivery. A genuinely missing lower bracket remains an explicit failure, and
original ASIO intervals are never replaced by a midpoint. Network startup keeps
its existing pre-arm lower-observation seed.

Negotiated device callback bounds govern local observation preparation only.
Preserve the user's network setup deadline and session-clock maximum age on
every platform; device latency must not silently widen these independent
network policies. A configured deadline that cannot accommodate the selected
device remains an explicit startup refusal.

## Current development evidence and limits

The focused native-audio library/bin run passes 36 tests: 14 startup, 14 gameplay
and eight owner fixtures. Native-end and native-start filters pass 15 and 20
tests respectively. One joined native capture codec/reconstruction/replay fixture
preserves actual judgment events, hash, gauge, EX, logical timestamps, original
HOST provenance and replay keysound identity/order. These filters overlap and
are not a full-suite count. The current Node browser regression passes 524 tests;
it does not replace actual-browser QA.

The launcher macOS all-target Rust check passed using C/archive stubs before
two warning cleanups; it is source/type evidence, not actual SDK or hardware
execution. The first Windows launcher check failed with 39 import/scope errors;
the repaired public imports pass the C/archive-stub all-target recheck. Fresh
browser WASM checking passes. App library/bin regression with desktop and
WebTransport passes 2,175 tests with two existing ignored tests; legacy
capture/playback/native-feed integration regression passes 21 tests with
unchanged assertions. Fresh full-workspace checks, formal independent review and ordered CLI,
desktop and browser QA remain pending. Correlation accuracy remains explicitly
unknown. No AC, migration task or broad player Goal is declared complete.
