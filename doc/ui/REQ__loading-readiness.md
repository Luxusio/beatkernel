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
