# Keep temporary UI contention out of native startup failure

PlayerPublisher capability advertisement is cold game-owner setup. It now waits
for the output-control critical section rather than returning WouldBlock and
aborting a successfully opened Linux session. It checks cancellation before
waiting and again after acquiring the lock; cancelled setup does not publish
capability or report a gameplay failure. A closed noncancelled channel and a
poisoned mutex remain explicit errors. No audio callback acquires this mutex.

PlayerViewer idle reply polling reads the atomic busy flag and returns without
locking. Active command/reply paths retain nonblocking try_lock and correlated
settlement behavior. Temporary reply contention does not repeat a replacement.

Four regression tests cover idle polling under a held lock, startup contention,
cancellation during contention and closed/poisoned errors. Before the source fix
three failed and the closed/poisoned case passed; after the fix all four passed.
The full Player test group executed: 57 passed / 1 failed. That room-presentation
failure is named in the prior baseline. Existing three channel settlement tests
also passed, including controlled unwind. All five live-output desktop tests
passed again. Workspace all-targets WebTransport compilation and whitespace
checks completed with exit zero; only existing unused-code warnings remain.
Hardware/native callback acceptance, other known
defects and full independent review/QA remain outstanding.
