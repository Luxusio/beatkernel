# Live output action availability and local input feedback

Pressing F2 while live output controls were unavailable left a persistent
session failure, even though the worker continued normally. The open action now
checks the same availability predicate used by the UI hint before preparing a
child route. Unsupported, network, replay, cancelling, terminal and unacknowledged
pause states leave the play instance and any existing session diagnostic intact.
The route preparation guard remains in place as a separate admission check.

Clipboard and IME rejection previously discarded the edit without reporting
its error on LiveAudio. The shared text error route now updates that draft's
message. Rejected edits retain both editor and settings; successful subsequent
edits clear the local error. Session errors are unaffected. Existing screen
identity and stale clipboard cancellation still govern publication.

Coverage extends the actual F2 handler fixture with network/running play and
unchanged session diagnostics. A new fixture exercises clipboard completion and
IME commit, including rejected multiline input and recovery with a valid endpoint.
These are application routing tests, not real OS clipboard/IME acceptance.

Before the source fix, focused live-output tests reported 6 passed, 2 failed:
the unavailable F2 diagnostic and missing local edit error were both reproduced.
After the fix:

- Focused live-output desktop tests: 8 passed, 0 failed.
- Full main executable tests: 203 passed, 17 failed. Failure names exactly match
  the preceding output-reply baseline; the new regression passes.
- Workspace all-target webtransport check: exit 0, with existing library warnings.

The Harness task remains open; independent review and required QA remain pending.
