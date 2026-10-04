# Direct Worklet report transport component

Extend the bounded direct audio endpoint with actual report polling. Commands
and poll share one pending operation and one sequence, with explicit operation
correlation and no hidden queue. The Worklet builds reports from the actual
BrowserAudio report_word getters through one shared host/direct path outside
process(). Successful client poll validates the exact fixed report envelope;
empty/unavailable data is not invented rendering evidence. Existing Rust report
semantics remain authoritative, and all errors preserve permanent owner fences.

Production and four independently authored deferred fixture groups are written:
client +2 (5 total), Worklet +2 (15 total). Prior groups remain, with the old
direct-port poll refusal changed to a still-forbidden finish operation. Deferred
fixtures cover shared operation ordering, exact/unavailable reports, malformed
backing buffers, real shared report reads, terminal/error/timeout/close and stale
callbacks. Scoped whitespace checks found no diagnostics. No JavaScript parsing,
assertions, tests, generated bindings, runtime, formal review or QA was executed.
This is JavaScript-only, so unchanged successful Rust checks were not repeated. This
bottom-up component leaves the current Window polling caller intact until its
next dependent Worker integration. Browser/audio execution and measured input,
render and main-thread performance remain unverified. The full Goal is active.

Known ceiling: Window polling and presentation observation remain current caller
behavior until the next direct-report Worker integration. The resizable-buffer
fixture explicitly requires a JavaScript engine with resizable ArrayBuffer
support when it is executed later; current fixture assertions are unexecuted.
