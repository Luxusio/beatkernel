# Browser split driver uses common startup waiting

RoomClientDriver::take_start now advances RoomStartWaitState through a borrowed
port over its real RoomPlayClient. Original committed schedules are retained as
values until Ready and extracted once. Pending returns no schedule; cancellation
fences further extraction. Existing driver first-error/lifetime guards run before
sticky ready state, and close drops wait state and the transient schedule together.
The unchanged WASM take_start method and Worker caller therefore use the shared
startup policy without another API or JavaScript policy implementation.

## Known ceiling

JavaScript admission/prepared timers and asynchronous callback/cleanup remain
adapters. This does not complete browser RoomNetworkActor/RoomCompetition
orchestration, full BMS requirements, actual transport/timing or performance
acceptance. Fixtures, source checks and text inspection cannot prove operational
startup, physical sync or SQLite-grade test reliability. Test execution,
generated bindings/browser/native acceptance and formal review/QA remain deferred.

Three additional pure driver fixture groups (thirteen total) preserve pending
protocol/write evidence, compare the genuine schedule field-for-field against
the actual common owner and verify one-shot extraction, Leave, close and first
failure guards before/after Ready. Prepared-stage assertions strengthen the real
clock/start helper without removing existing assertions. No private state is
seeded and no synthetic schedule is used; assertions remain unexecuted.

Both writers stopped before scoped Rust formatting and four sequential
compile-only checks, all exit zero: workspace/all-targets with WebTransport,
no-default-features WebTransport/all-targets, WASM browser/library and WASM
browser-audio/library. Existing unused-code warnings remain. Host checks compile
pure fixtures; WASM checks compile the actual unchanged binding's driver calls,
not fixture children or generated JS. No assertions or formal acceptance gates
ran. Original browser integer conversions/caller were inspected as text only.
