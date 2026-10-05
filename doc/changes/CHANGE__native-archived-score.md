# Connect native completion to exact score archives

The current increment connects native solo/local completed recording composition
to version-2 score/timing archives. Solo scoring observes actual committed runtime
reports through a statically composed host observer; local finalization borrows
the existing independent member summaries. No UI readback, replay rerun or
score-derived completion is used. Typed completion and original-ID association
remain required. Legacy scoreless public entry points retain version-1 behavior.

The score observer attempts underlying report publication even when scoring
refuses, retaining the original score failure first. Other host commands and
completion publication pass through unchanged. Finalization prepares archives
before capture consumption and attempts replay saves before archive writes while
preserving original gameplay, cleanup and publication errors.

## Known ceiling

At this increment native Records preview of stored final score details remained
unfinished. [The following retained detail increment](CHANGE__native-record-details.md)
connects that preview; [the grade-page increment](CHANGE__stored-grade-pages.md)
adds native grade-table navigation. Browser interactive paging and archived
saved-opponent comparisons remain unfinished. First observation of a new grade may allocate
the existing score-map node; cold archive staging/encoding can allocate. No
benchmark or global allocation-free claim is established. Fifteen independent
fixture groups are authored: six genuine Runtime/static host delegation and
scoring/publication refusal groups, six archive/finalization groups, and three
actual portable pump/Mixer completion groups. Numeric archive boundary values
and typed proof in save-policy fixtures are explicitly injected; actual pump
fixtures separately exercise genuine completion. Existing fixtures remain intact.
Both writers returned terminal stop reports before scoped Rust formatting of
shared sources and fixture children. OS roots retain surgical call-site edits.
Four sequential compile-only checks exited zero: workspace/all-targets with
WebTransport, no-default-features WebTransport/all-targets, WASM browser/library
and WASM browser-audio/library. Existing unused-code warnings remain. Host checks
compile the new fixture children and Linux native entry point; WASM library
checks compile the shared library without its test children. No assertions ran.
Windows/macOS adapter call-site edits require later target compilation and
hardware acceptance. Assertions, native/browser acceptance, formal review,
required QA, verify and close remain deferred. The full player Goal stays active.
