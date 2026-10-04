# Native room competition and startup adapter

`NativeRoomCompetition` now adapts the actual `NativeRoomNetwork` to the existing
`NativeStartAgreement` interface. Its production port delegates to that owner;
it adds no room protocol, network thread, dependency or crate. Explicit Seal,
Ready and Leave requests retain bounded correlated replies. Awaiting commitment
services the caller's native acquisition/cancellation callback and accepts only
the common committed schedule. Pending Leave, cancellation and retained failed
commitment fence output startup. Host brackets preserve the network owner's
original clock rather than constructing an audio origin from readiness.

Actual Prepared membership initializes the existing `RoomOpponentHud`. Qualified
peer prefixes update only when their sequence changes, and pagination reuses its
four-row page model. Prefixes accepted in the same poll as a failure reach the HUD
before disconnect. HUD failure disables presentation alone. Actual ordered local
progress is validated and retained before optional publication, with a 50 ms
original-clock cadence and one outstanding ordinary publication. Backpressure
and post-commit network failure preserve local progress; invalid local progress
still refuses explicitly.

Natural finish requires caller-proven completion and previous actual local
observations. It waits for final-publication admission, explicitly requests
coordinated Drain, then waits for an actual terminal outcome including stream
cleanup before joining. Four historical receipt flags alone do not satisfy this
wait. The fixed finish timeout is not renewed. Cancellation/unplayed setup sends
no fabricated final or drain. Cleanup-only failure stays in cleanup_error rather
than being relabeled as a protocol failure. Joining retains queued command
refusals and the accepted comparison data; repeated finish returns its retained
outcome. Drop cancels and joins without inventing success.

Independent deferred fixture source adds six groups. The scripted boundary is
`NativeRoomPort`; registry, clock, start coordinator/agreement and progress
client/relay generate the actual protocol evidence. Groups cover lobby
correlation, qualified HUD paging, genuine schedules, publication backpressure,
final/drain evidence, cancellation, fixed deadlines and cleanup separation.
They have not been executed.

Both writers actually stopped before scoped rustfmt and whitespace inspection.
Four compile-only Cargo checks exited 0: workspace/all-targets with WebTransport,
runtime no-default/all-targets with WebTransport, WASM browser and WASM
browser-audio. Three existing cadence dead-code warnings remained on WASM.
No tests, JavaScript parsing, applications, generated bindings, devices/network
runs, formal reviews, QA, task verification or close were executed.

Known ceiling: native application selection, interactive lobby controls, the
native player/HUD bridge and actual solo/cohort activation/finite-finalization
callers are not yet wired to this adapter. Existing bilateral app routes are
unchanged. Custom ports must honor the native owner's bounded acquisition, clock,
command and joined-cleanup contract. Live transport/browser/device behavior and
measured performance remain unverified. The full Goal and Harness task remain
open; source implementations and compilation cannot establish runtime PASS.
