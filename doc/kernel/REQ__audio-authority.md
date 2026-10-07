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
