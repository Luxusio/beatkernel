# Keep pending room publication before clock access

An unacknowledged Progress command once again guards ordinary publication before
clock sampling. The native/common controller still validates and retains each
local prefix, but cannot admit a second Progress while the first is pending.
The misplaced pending-Progress condition is removed from room UI closure:
publication backpressure does not close an otherwise Connected room.

Two prior regression tests failed before the fix, and a new attached-UI test
also reproduced the unwanted clock access. All 32 RoomCompetition tests now
pass, covering unchanged local ordering, admission credit, failure/Leave/final
handling and correlated UI effects. The new test verifies no extra clock access
or network command, retained latest local progress and Connected status.
Full library execution reports 1465 passed / 98 failed, reducing the prior
failure count by two; every remaining failure is named in the supplied baseline.
Workspace all-targets WebTransport and WASM browser checks plus whitespace
checks exited zero, with only existing unused-code warnings. No actual QUIC/
WebTransport/server or native startup acceptance is inferred; full independent
review/QA and remaining Goal work stay outstanding.
