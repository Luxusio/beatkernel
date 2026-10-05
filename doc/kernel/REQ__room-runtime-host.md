# Injected room waiting and cleanup diagnostics

RoomRuntimeHost owns creation of startup and final wait controls plus reporting
an implicitly dropped room outcome. Its associated controls implement the
existing RoomStartWaitControl and FinalWaitControl contracts with IO error
values. The contract contains no Instant, thread, socket, player or renderer.
Startup control creation is a cold, effect-free handle operation; actual waiting
occurs only when the existing startup policy requests it. Final control creation
receives the validated fixed finish timeout and returns its independent u64
deadline. A control must measure the whole final wait, without renewing the
deadline on queue pressure or polling.

NativeRoomCompetition accepts a runtime host through new_with_ports, alongside
its existing network and UI hosts. Existing new and new_with_host constructors
keep native runtime defaults. The controller preserves startup observation
ordering, cancellation/error side effects, original service errors, room-clock
deadline arithmetic, final ACK/drain admission and actual joined outcome handling.
It creates a final control only for genuine natural finish after observing a
committed game; cancellation and unplayed setup join without a terminal prefix.

NativeRoomRuntimeHost alone creates checked Instant deadlines and native sleep
controls. Drop requests stop and joins once when no outcome was retained; it
reports that actual joined outcome through the host. The native host retains
the existing cleanup-error-before-network-error diagnostic and message. Explicit
finish followed by Drop performs no extra join or implicit-drop report.

The generic controller adds no per-note allocation, lock or dynamic dispatch.
Fake associated controls can inspect timing, refusals and lifecycle independently
of OS clocks. Independent deferred fixtures exercise actual controller startup,
natural finalization, timeout/control refusal and explicit versus implicit cleanup.

## Known ceiling

The controller remains in its native compatibility module with concrete defaults
and the native start-agreement implementation. Worker IO, full portable controller
construction and the remaining result presentation separation are unfinished.
Compilation and fixture authorship do not prove actual adapter equivalence,
physical timing, successful peer receipts, test stability or measured performance.
Runtime tests, hardware/browser/network acceptance, formal review and QA remain
deferred under the existing user instruction.
