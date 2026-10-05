# Shared retained mine rendering

PlayerChart retains exact compiled mine metadata separately from ordinary notes:
full-width source ordinal, ordered lane index, timestamp and typed damage.
Visible-mine queries use binary timestamp bounds and reusable index storage;
their independent 2048-marker budget rejects dense windows explicitly rather
than truncating the timeline. Empty sources need no mine-query allocation.

Scene prepares note and mine queries before committing its common playfield
packet. PlayfieldCache retains both visible sets and emits one existing head
primitive per mine, red for nonfatal and magenta for instant death. Ordinary
head/body/tail geometry remains separate. The common instance buffer covers
three normal primitives per normal-note cap and one primitive per mine cap.

Stable visible sets reuse the existing instance Arc and local-epoch GPU drift;
metadata, geometry, lookahead, visible-set or backward-time changes rebuild the
packet. Solo and paged-local browser/native composition share this Scene path.
No Window render loop, gameplay decision, score event or synthetic judge advance
is introduced.

Five independent deferred fixture groups cover parsed metadata/normal-progress
separation, inclusive wide-window linear-oracle equality and scratch budgets,
retained geometry/colors/cache invalidation, actual solo plus all 32/64-player
pages, and Scene density/lane rejection before packet mutation. Two groups are
portable and three use the existing graphics feature. Both source and fixture
authors returned actual terminal STOPPED before scoped formatting and checks.

Scoped Rust formatting and whitespace checks completed. Four locked compile-only
configurations exited 0: workspace/all-targets with WebTransport, headless
WebTransport/all-targets, WASM browser/lib and WASM browser-audio/lib. The retained
normal-only cache wrapper now produces one unused-method warning in graphics
library configurations; existing WASM cadence unused-code warnings remain.
These checks do not establish Windows/macOS target-specific acceptance.
No tests, formal review/QA or overall task/Goal completion are claimed.
GPU/browser/device execution and measured performance remain unverified. Complete
mine source admission, gauge/fatal-stop policy and WAV00 integration remain
unfinished; the shared preparation guard remains enabled.
