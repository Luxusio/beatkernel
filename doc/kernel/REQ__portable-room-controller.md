# Portable room competition controller

RoomCompetition<P,H,R> belongs to an unconditional platform-common module. Its
production imports use portable room values, RoomNetworkPort, RoomUiHost and
RoomRuntimeHost without native owners, concrete defaults, player globals,
physical output adapters, threads or Instant. Actual lobby correlation, roster
and prefix admission, retained HUD/UI publication, progress cadence, natural
final/drain policy and explicit/implicit cleanup remain one shared implementation.
Peer scores remain presentation data and never enter local judgment.
The crate-private HUD operation retaining accepted prefixes after join is also
platform-common. Its participant, sequence, finality and score validation remain
unchanged; only the obsolete native-only availability gate is removed. Ordinary
live updates still refuse disconnected HUD state.

new_with_ports accepts all three hosts explicitly. Roster and timeout validation,
bounded reservations and failure cleanup retain existing behavior. Startup is
exposed through await_commit_with_service with an ordinary generic callback and
original service error payload. Its typed RoomStartWaitError distinguishes
port, control, service, closing, pending Leave and terminal failure. Diagnostics
retain the existing messages and first-failure behavior without wrapping or
replacing the returned original service error. Cancellation requests stop and
marks the retained HUD disconnected in the same order as before.

committed_schedule_value returns the original schedule only while the existing
active-state guards hold; room_clock_ns reads the port's original room clock.
Neither accessor manufactures readiness or proves physical timing. Fixed final
room/control deadlines, genuine final admission and observed receipts continue
to guard natural finalization. A joined cleanup error remains independently
retained even after successful peer drain.

The native compatibility module alone defines NativeRoomCompetition with its
existing default hosts, the legacy new_with_host constructor, NativeRoomPort
alias and NativeStartAgreement implementation. Its service error conversion
preserves the original boxed payload; host-bracket sampling and physical output
start adaptation stay outside the portable controller. Existing native new
continues to use that compatibility boundary.

Existing fixture files remain unchanged and are registered under host-only test
compatibility imports within the controller so private-state regression coverage
is retained. New independent fixtures import only the portable controller/ports
and exercise actual startup, error ownership, cancellation, schedule guard,
constructor refusal and one-time cleanup. WASM library compilation now includes
the actual shared controller, but not test execution or a browser adapter.

## Known ceiling

Worker stream/thread/clock implementations, real network delivery, browser room
adapter integration, hardware timing and cross-platform/performance acceptance
remain incomplete or unverified. Cold retained result presentation still lives
in controller policy; extraction introduces no new queue, lock, dynamic dispatch
or per-note allocation, and supplies no measured zero-cost or SQLite-grade
reliability evidence. Tests, formal review and QA remain deferred.
