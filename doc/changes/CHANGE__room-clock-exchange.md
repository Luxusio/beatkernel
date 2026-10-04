# Prepared-room clock exchange

The common BMS app adds a transport-independent clock owner for each stream's
actual admitted participant in a validated Prepared multi-host room. It reuses
bilateral ClockProbes/ClockFilter through typed fields while retaining existing
BKMP bytes. BKMR sequences 1..8, original four timestamps, exact complete-write
receipts and bounded pending state establish the evidence required before an
estimate can enter shared software-start coordination. An early genuine Pong
retains its original receipt time but cannot bypass complete-write barriers.
Rejected operations preserve state and chronology; stopping fences readiness.

Independent deferred fixtures cover actual paired exchanges, original clock
intervals, write/sequence/chronology barriers, prepared membership and boundary
times. Compile-only evidence will be recorded in the task checkpoint after
writers stop. No test execution, TLS session, browser, device or audio evidence
is claimed. The WebTransport server and room client still refuse clock/start
controls until this owner is composed with actual stream acquisition timestamps,
full-write receipts and RoomStartCoordinator/StartAgreement. Worker activation
remains fenced until a real committed output schedule is available. The full
BMS player Goal and formal review/QA requirements remain open.
