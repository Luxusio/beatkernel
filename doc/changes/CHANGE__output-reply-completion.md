# Preserve decided output results through cancellation and owner exit

An output replacement can complete joint publication before the user cancels.
The previous Player synchronization adapter settled the in-flight request as a
cancellation before admitting its decided result. In addition, retryable reply
publication could leave a success buffered in the bridge if the gameplay pump
exited before the next retry. The user then saw a cancellation or session-end
error for an output configuration that had actually become active.

The pure OutputControls state now distinguishes cancellation from owner close.
Cancellation stops admission and settles queued requests, retaining any in-flight
identity for its owner's result. Owner return/unwind still closes and settles
undecided work. UI polling and rejected requests cannot decide an in-flight
outcome prematurely. Decided result admission precedes cancellation settlement;
terminal cleanup preserves the correlated reply.

The statically injected PlayerOutputUi adapter uses cold blocking publication of
an already-decided result. This waits for the bounded UI control critical section
before returning to the pump, so its result cannot remain buffered when that
pump exits. Existing nonblocking Player reply calls and generic retryable test
ports remain available. Request acquisition, UI actions and idle polling keep
their nonblocking behavior. No mutex operation is introduced in audio callbacks.

Regression coverage uses actual Player channels and the real PlayerOutputUi
adapter: cancellation with/without a UI poll, owner failure after a decision,
undecided in-flight settlement on owner return, and a mutex held by the UI while
a worker publishes a changed applied endpoint and then exits with an unrelated
error. No sleep-based race timing is required.

Verification (2026-10-06):

- Before the fix, the new decided-reply cancellation regression failed: the
  expected success was replaced by an output-controls cancellation error.
- Actual Player output channel tests: 6 passed, including 3 new regressions.
- Full library: 1474 passed, 94 failed. Failure names exactly match the preceding
  FLAC baseline; 3 added regressions pass.
- Main executable tests: 202 passed, 17 failed. Failure names match the preceding
  viewport/startup-focus baseline.
- Workspace all-target webtransport check and WASM browser library check: exit 0.
  Existing dead-code warnings remain.

The broad Harness task remains open; required independent review and QA,
including browser QA, remain pending. Foreign-call blocking and native-device
acceptance are separate unfinished work.
