# Native and browser loading readiness

Loading must keep input acquisition and visible UI updates usable. Preparation
readiness, an available chart path, a submitted usable frame and native/audio
output readiness are separate facts; none substitutes for another.

Native catalog scanning shall publish admitted canonical paths in batches of
at most 64 entries through a channel of capacity two. UI polling drains at most
two batches without waiting. The partial list remains selectable while later
metadata and title-font work continue. A completed worker remains owned until
its final font or error result is consumed; thread completion alone must not
discard that result, including completion between UI polls. Sorting, metadata
and font publication
preserve the selected path, query text and focus; font upload remains UI-owned
and cannot reset a different route. A chart becomes playable only after its
actual complete preparation and fresh setup validation.

Browser preparation shall expose actual stages near Prepare and an explicit
keyboard-accessible Cancel action. Previous preview/catalog remains usable
after current-owner cancellation or failure. Library replacement, settings,
play and navigation cannot revive older work. Progress reflects actual work
and never fabricates a completion percentage or PCM-ready catalog status.
Native preparation stage, resource counts and cancellation guidance occupy a
readable row separate from the play status, practice caption and controls;
their glyphs must not overprint one another in the standard player window.
Each preparation advance handles at most one resource phase before yielding;
a library borrow must not survive that yield.

Cancelling during an unabortable file read also releases that preparation's
waiter and completes its runner before the read settles. Repeated cancellation
of the same held read must not accumulate subscriptions or retired runners.
The single bounded underlying acquisition may settle later, with current-owner
validation required before hydration and no access to a retired WASM library.

Selected-file jobs validate both inventory owner identity and that owner's
revision. Two different inventories with equal revision numbers are different
libraries; neither can provide media for, or admit, the other's pending job.
Moving an inventory preserves its owner identity. Identity never substitutes
for exact resource bytes when deciding content reuse.

Dependency lookup and subsequent preparation reuse the same immutable parsed
original when source identity, seed and chart bytes match. Continuation still
validates current limits and replay setup before reading assets. Query parsing
and compilation count as preparation work in measurements.

Browser cache publication occurs only after fresh visual, section, policy and
replay preparation succeeds and the current library admits the result. Moving
the prepared value into that validation must preserve a separate revocable
admission handle. Failure, cancellation, abandonment or stale admission keeps
the previous cached original; consuming a prepared result does not publish it.

Native chart preparation admits its audio original after fresh section, lane
binding and play-policy validation. Production replay preparation also validates
the derived recording before admission. Later uncached native visual registration
has separate ownership: failure keeps the validated audio original reusable,
while playable and output readiness remain false. This avoids retaining a
transaction across native setup stages.

Native reload/replacement retires unfinished jobs, polls them to terminal and
joins only after terminal observation during ordinary UI interaction. At most
two retired jobs are retained; capacity exhaustion reports cleanup-pending and
keeps the UI operable. Cancellation-aware producers must not remain blocked on
a full/disconnected metadata channel. Exit cleanup owns its explicit join.

Verification requires held scan/read/font cases, partial-path selection,
path/query/focus continuity, stale completion refusal, cancel/error/recovery,
actual trusted input and submitted frames in current native/browser execution.
A single syscall, parse/compile phase or codec call can remain indivisible;
report its measured tail instead of promising preemption or instant completion.
General narrow-screen layout and physical device performance remain separate
requirements.

Loading reports consume observed trials through `tools/loading_report.py`.
Dense-chart preparation is a separate workload from large encoded audio. Its
deterministic 20,000/100,000-note charts share a small WAV, stay inside parser
object and physical-line limits, and require actual prepared note/sample counts.
Each variant/density has at least five fresh cold process/library/cache owners
and a warm retry on that same library. Reports separate actual readiness from
request-correlated submitted frames and retain unavailable historical metrics.
Natural trusted input is classified by its observed time relative to readiness;
an event after readiness cannot prove input acquisition during preparation.
The dense-only measurement does not require or replace genuine replay measurements.
Before/after groups use the same workload, environment, fixture and configuration
hashes, with at least five trials per group and explicit artifact provenance.
Cold trials use distinct process/library/cache owners; warm trials identify the
owner actually reused. Events use relative monotonic observation times and the
same request correlation. Catalog availability, prepared resources, submitted
usable frames and audio readiness remain separate measurements. Input/redraw
latency samples require actual observed events. Missing historical phases or
counters stay unavailable and cannot produce a before/after delta. Statistics
use nearest-rank p50/p95 and observed maximum, with sample counts reported.
The report validates record consistency; the runner must independently observe
artifact bytes and event origins. CLI fixtures validate report mathematics and
refusal behavior and are never player performance evidence.

Cooperative preparation yields must permit current input/cancellation and Ready
admission fencing without adding avoidable nested-timer delay. A preparation
may own one MessageChannel with at most one pending wake; completion or
cancellation closes both ports and wakes the current waiter. It must retain a
macrotask boundary before admission and cannot retain queued callbacks or
preparation owners after cancellation. Actual before/after timing and held-work
input/cancellation verification accompany scheduling changes.

Preserved native executable comparisons must observe the actual owned window
without injecting newer Desktop hooks into the old binary. A confirmed external
selection change and restoration may prove catalog usability; frame observation
times are upper bounds that include sampling and observer cost. They remain
distinct from current Renderer submission timestamps and audio readiness.
Catalog-only comparison and current-only injected font gates do not establish
native playable-ready, section/replay or replacement comparative performance.

An externally observed native preparation marker must be emitted by both
preserved production variants after their real original and derived preparation.
Correlate each marker to its actual owned-window launch/retry input; include
stdout observation and input-delivery overhead in the reported upper bound.
Subsequent output/input setup failure remains a separate observed endpoint.
A joined technical failure may permit a real same-owner retry and retain a
validated original, but cannot establish playable or output readiness. Missing,
duplicate or uncorrelated markers invalidate that trial.

Replay performance fixtures must state whether recordings contain accepted
inputs and a recorded horizon. For nonempty execution evidence, obtain trusted
input through actual playback, wait for correlated input acknowledgement and
advancing audio-owned presentation before stopping, and validate the saved
capture. A startup-only empty capture proves preparation/setup alone. Never
substitute a sleep, host-time estimate or audio-ready setup acknowledgement for
an observed playback frontier.
