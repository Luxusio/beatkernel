# Browser presentation status recovery

The Window displays transient graphics-wait feedback while its current visual
owner cannot present. A matching actual `drawn` event restores the preceding
ordinary status text and error flag. Renderer initialization, geometry replies,
state acceptance and audio progress do not prove presentation.

Keep at most one pending restore identity: Window owner/Worker, selected chart,
optional active play, visual generation and content. Invalid or older identities
cannot replace a newer pending wait. Preparing, closing, shutdown, navigation
and Worker replacement prevent an old visual result from changing current
feedback. Live/replay/local messages must belong to the active play; an old
play message cannot restore preview or Results feedback.

Menu presentation evidence carries its existing menu generation, screen and
revision because menu navigation can reuse a visual generation. The game owner
and Window reject evidence for an older menu state. Accepting navigation
cancels an obsolete wait overlay and restores ordinary feedback; that action
does not assert that the new screen has already presented.
Menus and saved historical results can display before preparing a chart.
Selected chart identity zero means there is no prepared preview; it is valid
for the current visual owner. Visual generation/content and optional play/menu
tokens establish presentation ownership independently from chart selection.

Repeated wait events preserve the same preceding status. Every ordinary status
publication, including identical text or a new error, invalidates pending
restoration. A later drawn event must retain that newer status. Restoring a
preceding error retains its error flag; successful graphics output does not
claim that another application operation succeeded.

An idle or duplicate drawn event performs no status DOM update. This is a small
event-driven state helper, without timers, frame clocks, model reconstruction
or virtual DOM. Gameplay, audio, input and Renderer Worker ownership stay with
their existing layers. Verify pure status transitions, actual main-module
message wiring and real browser surface wait/recovery independently.

## Executed evidence — 2026-10-09

Independent DEEP code review passed. The complete Node suite passed 777 tests
with zero failures. Independent Chromium QA passed sixteen scoped interactions
using real zero-extent waits and subsequent GPU presentation, including exact
baseline/error restoration, later feedback preservation, menu revisions and
Back, preview/live/replay/local-two recovery, and first-run menu/history with
no prepared chart. No console or HTTP errors occurred; deliberate reload
request cancellations were retained as lifecycle evidence. The browser and
owned HTTPS server exited. Reproduce the Node suite with
`node --experimental-vm-modules --test --test-concurrency=1 app/web/*.test.mjs`.
Browser transcripts and screenshots are ignored artifacts under
`target/wf/browser-render-feedback/qa-browser`; pure/Window/renderer tests remain
in Git. Rust and the current browser WASM package were unchanged by this task.

The graphics checks do not establish complete replay correctness. QA preserved
a six-second replay that fails with `completion presentation precedes its
output frontier` and reproduced it using production JS from baseline
`3aa4ccb`. The recording, trace and steps are in the QA artifact
`replay-followup.md`; correcting that audio chronology remains a separate player
Goal task. A stale menu token after playback was also observed and requires
its own lifecycle followup. Neither issue is counted as fixed here.
