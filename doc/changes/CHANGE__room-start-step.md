# Resumable room startup waiting

RoomStartWaitState performs the existing initial/service/observation policy one
iteration at a time. Pending calls return a scheduling delay; the existing
await_room_start wrapper alone waits. Initial observation happens once, with
cancellation/closing precedence preserved. Service and port errors retain their
original associated values. Terminal success/cancellation repeats without
further effects; any failure seals the state against reentering the service or
port. The state cannot be copied or reset to duplicate wait authority.

## Known ceiling

The native controller uses the common state through its existing wait wrapper.
Actual browser startup wait-state orchestration is still unfinished, while
protocol admission/clock/start agreement already lives in common Rust owners.
Fixtures are authored but unexecuted. Real scheduling, browser/network/native
acceptance and performance, formal review and QA remain deferred. No completion
of the full player or SQLite-grade reliability is claimed.

Six independent pure fixture groups cover all initial gates, all 32 poll
precedence combinations, repeated pending initial-once traces, service
cancellation/opaque identity before polling, unsized callbacks and wrapper-only
waiting with original control refusal. Repeated terminal steps explicitly check
that service and port effects do not recur. Assertions remain unexecuted.

After both writers stopped, scoped formatting and four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, no-default-features
WebTransport/all-targets, WASM browser/library and WASM browser-audio/library.
Existing unused-code warnings remain. Host checks compile both original wrapper
fixtures and new finite-state fixtures. WASM library checks compile portable
policy, not fixture children or runtime behavior. No tests, browser/native IO,
formal review, QA, verify or close gates were executed.
