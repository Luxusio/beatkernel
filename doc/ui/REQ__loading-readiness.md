# Native and browser loading readiness

Loading must keep input acquisition and visible UI updates usable. Preparation
readiness, an available chart path, a submitted usable frame and native/audio
output readiness are separate facts; none substitutes for another.

Native catalog scanning shall publish admitted canonical paths in batches of
at most 64 entries through a channel of capacity two. UI polling drains at most
two batches without waiting. The partial list remains selectable while later
metadata and title-font work continue. Sorting, metadata and font publication
preserve the selected path, query text and focus; font upload remains UI-owned
and cannot reset a different route. A chart becomes playable only after its
actual complete preparation and fresh setup validation.

Browser preparation shall expose actual stages near Prepare and an explicit
keyboard-accessible Cancel action. Previous preview/catalog remains usable
after current-owner cancellation or failure. Library replacement, settings,
play and navigation cannot revive older work. Progress reflects actual work
and never fabricates a completion percentage or PCM-ready catalog status.
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
