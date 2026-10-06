# Share the complete browser room binding preflight

Worker room admission checked an older subset of BrowserRoomClient methods,
while BrowserRoomOwner required additional publication/setup/frame/drain APIs.
Missing capabilities could therefore be detected only after constructing a
session, rather than through the recoverable pre-acquisition path.

Export one frozen ROOM_SESSION_METHODS list from room-owner and use it for both
owner configuration and Worker prototype/instance checks. It includes cadence
and delegated setup/frame/drain waits. Reject missing prototype capabilities
before constructing the room client; reject malformed instances before ownership
transfer and clean them with the existing close/free path. Preserve gameplay
and avoid opening transport on refusal. No fallback binding or local replacement
for common Rust policy is introduced.

The Worker test's fake WASM room object now provides the current delegated wait
interface, retaining controllable setup/frame/drain state and deadlines. Expand
missing-method cases to the seven newly required methods. These scripts are mock
boundary behavior, not independent proof of Rust timing policy. Actual common
Rust behavior is separately covered by the library suite.

The mixed HID/contact setup test verifies both configurations precede capture,
rather than requiring an undocumented touch-before-HID order. One completed-room
fixture removes copied references to an undefined options variable and uses its
actual default committed-room setup. Other outcome assertions remain.

Verification (2026-10-06):

- Worker tests with experimental VM modules: 99 passed, 10 failed, compared with
  the preceding 84/25 baseline; 15 resolved and no new failing names.
- Room owner tests: 37 passed, 0 failed.
- Independent code review: PASS, DEEP bounded formal-only, no findings; targeted
  Worker 3/3 and owner configuration 1/1 independently executed.
- Independent security review: PASS, no findings; room owner 37/37 and targeted
  Worker 2/2 independently executed, including frozen-list mutation rejection.

Review scope is the current production preflight/test/REQ change. Full Worker
suite, actual WASM/WebTransport/browser and complete task acceptance remain
unfinished. No coordinator-authored receipts or task close is claimed.
