# Resumable room final waiting

RoomFinalWaitState advances the existing final/drain policy one iteration at a
time. Pending calls return a bounded scheduling delay without parking; the
existing native blocking wait uses the same state and parks in its wrapper.
Own admissions, observed acceptances, genuine receipts and both fixed clock
deadlines remain required. Success and failures seal the state against repeated
port effects; opaque errors are returned unchanged on their first occurrence.
The state cannot be cloned to duplicate command authority.

## Known ceiling

This is a policy component used by the existing blocking wait, not completed
browser asynchronous integration. Browser room-owner lifecycle and timers remain
adapter work. Scripted fixtures and compilation cannot prove transport delivery,
physical timing, scheduling latency or allocation/performance measurements.
Test execution, runtime acceptance, formal review and QA remain deferred.

Seven independent pure fixture groups are authored and compiled only: admission
pressure and persistent state, receipt/terminal gates, dual fixed deadlines,
cross-step clock regressions, original opaque errors and sealing, full-width
arithmetic/overshoot, and wrapper-only virtual parking with refusal.
After both writers stopped, scoped Rust formatting and four sequential
compile-only checks exited zero: workspace/all-targets with WebTransport,
no-default-features WebTransport/all-targets, WASM browser/library and WASM
browser-audio/library. Host checks compile fixtures; WASM library checks compile
the policy but do not compile fixture children. Existing unused-code warnings
remain. No assertions, runtime, review, QA or close gates were executed.
