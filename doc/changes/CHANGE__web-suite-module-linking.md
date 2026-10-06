# Restore complete module linking and current web fixture contracts

The Window harness omitted completed-results-model.mjs, blocking 124 test bodies
at import linking. The preview Worker stub omitted the newly shared
ROOM_SESSION_METHODS export, blocking another fourteen. Link the actual completed
helper and export the real frozen method list through the test stub; its room
constructor still refuses acquisition. No production helper is mocked away.

Remaining fixture corrections retain their original outcome checks:

- Normalize VM-realm arrays with outer Array.from while preserving exact
  player/own/label values and original File identity assertions.
- Account for the initial native frame read required to arm output. Inject later
  live frame failures after startup, while replay uses no later live input frame.
- Keep the exact PCM slot limit 7940 (62*62+4096) instead of a stale 5392.
- Activate contact input for the local archive's assigned touch source 2.
- Pass explicit converted text to Node TextEncoder.encodeInto as the string-only
  oracle; Worklet conversion, UTF-16 consumption, byte counts and untouched-byte
  assertions remain.
- Final comparison rows clear previous contents and then publish exactly one
  complete snapshot after both owners join; periodic messages still never render
  those results.

These are module/test compatibility repairs. Node/VM DOM and generated-binding
mocks do not establish real browser, Worklet or WASM execution. Separate production
roster target refresh is described in its own change document. Full task/Goal
completion is not claimed.

Verification (2026-10-06): full Node web suite with experimental VM modules and
test concurrency two reports 508 passed, 0 failed, exit 0, versus the initial
368/140 baseline. Actual Chromium is available locally; real-browser verification
requires generating the presently absent WASM package and remains separate.
