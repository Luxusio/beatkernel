# Common client drain integration

RoomPlay now exposes explicit request_drain and drain_complete through its
existing progress owner. It receives DrainComplete only after genuine committed
start, using the original capture and exact participant/final sequence. The
single external write-ID mapping retains child Ready receipts; an early matching
notice waits for the actual full write. Premature/repeated local requests refuse
recoverably, and Stop/Leave revoke completion without implicit transport close.

RoomPlayIo exposes the same operations and returns idle without clock or stream
access after genuine completion. It also skips the next read if a held notice
becomes authoritative on the Ready write receipt, preserving success until
explicit Stop. Caller-owned stream disposal is separate. BrowserRoomClient
exposes the same request/status with recoverable state error tagging and a
fatal-failure/close completion fence, without altering snapshot revisions.

Independent deferred fixtures add two RoomPlay groups (10 total) and two stream
driver groups (8 total). Actual committed client/relay composition covers
2/3/4/64 hosts, exact write IDs, capture floors, early Complete, atomic refusals
and Stop/Leave. Actual scripted streams cover partial/WouldBlock/Interrupted
Ready writes, fragmented Complete reads, completion without later clock/I/O,
completed-client handoff and cancellation. The synchronous stream seam cannot
exercise an early notice before its direct full-write callback; that ordering
is covered by RoomPlay without manufacturing transport causality. No fixtures
have been executed.

After both writers stopped, scoped Rust formatting and whitespace checks
completed. Four compile-only checks finished with exit 0: workspace/all-targets
with WebTransport, runtime/all-targets without defaults plus WebTransport, and
WASM library checks for browser and browser-audio. WASM checks retain three
existing cadence dead-code warnings. These checks do not prove runtime behavior.

Browser Owner/Worker automatic final drain and native application activation
remain pending; no runtime or QA acceptance is claimed. The full BMS player
Goal remains active.
