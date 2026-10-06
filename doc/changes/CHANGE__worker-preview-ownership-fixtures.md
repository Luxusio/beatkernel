# Separate catalog preview ownership from refused gameplay preparation

catalogWorker already selects a chart into the preview renderer before each
play-start request. That preview's prepared object is intentionally consumed by
BrowserView. Two older tests incorrectly assumed no prepared object existed,
or that every prepared object must remain unconsumed after refusing gameplay.

The local saved-target case retains the original preview list and verifies that
invalid member/solo targets add no gameplay preparation or game owner. The
pointer refusal case snapshots original preview ownership and free counts, then
checks only newly created gameplay preparations: none may be consumed, and each
must be released exactly once. Existing no-constructor/no-prepared-reply checks,
profile/source collision, binding capacity and exact-capacity positive cases
remain intact. No production preflight or consumption rule is relaxed.

This fixture-only change distinguishes two genuine ownership boundaries rather
than masking preparation transfers. Actual WASM/browser/native acceptance and
other full-Goal feature/QA work remain unfinished; the Harness task stays open.

Verification (2026-10-06): full Worker tests with experimental VM modules report
109 passed, 0 failed, exit 0 versus the preceding 107/2 baseline. All branch
assertions in the two formerly blocked tests now execute, including late saved
file-read cancellation and exact pointer binding capacity. This is Node with
mocked generated WASM bindings/browser APIs, not actual browser/WebTransport QA
or full player completion.
