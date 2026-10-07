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
Freshness applies to the selected covered observation as well as the newest
retained evidence. Its age at the declared query time must be nonnegative and
within the configured limit. An input occurrence or acquired prefix beyond
that declared current HOST time cannot authorize an operation yet; hold without
changing the queue, mapping history or watermarks.

Retire history only through the fully closed original HOST prefix while keeping
the most recent two real anchors. Never evict a segment needed by pending input.
`InputMerger::peek_ready` validates a frontier and exposes the earliest eligible
event without popping it or changing byte/sequence accounting. Prepare its
mapping before removal and dispatch it once through the real Runtime.

Preparation descriptors are immutable and record exact semantic state. Commit
rejects stale revision, wrong epoch and mismatched mapping/state, including a
descriptor prepared by another owner with different history. Equivalent exact
semantic state may accept; no global counter, hash, clock read or allocation is
needed. Rejections preserve every prior watermark and retained observation.

Output replacement requires a strictly newer epoch, unchanged HOST and logical domains,
logical origin at or beyond the committed operation and zero pending input.
Validate and prepare before committing; recheck pending input and staged state.
Retired observations cannot regain authority.
Retained acquisition watermarks belong to the same original HOST domain and
merger. A different HOST clock requires a new session and merger; output
replacement must not retimestamp that provenance.

Same-stream resume uses a separate correlation restart: retain physical epoch,
configuration and all acquired/closed/committed watermarks, clear only observation
history using reserved storage, and require two fresh pairs. Consumer lifecycle
owns Transport pause/resume; restarting correlation never resets judged history.

## Verification cues

Use pure memory fixtures with actual `InputMerger`: two-anchor startup, analytic
unequal-rate mapping, original input provenance, backward/forward permission
limits, prefix lag, equality ordering, predicted-input catch-up, stationary and
stale output, retained delayed input, pinned capacity and retirement, domain and
epoch faults, failed replacement, same-epoch restart, stale and cross-owner
descriptors. Assert complete state preservation on rejection.

These fixtures establish the boundary only. Shared Step/native/browser owners,
capture/replay and lifecycle still require migration and independent CLI,
desktop and actual current-browser QA. The previous browser playback failure
remains unresolved until those real production paths pass.

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
operation. These lifecycle integration rules are selected requirements; the
production consumer implementation remains pending.

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
