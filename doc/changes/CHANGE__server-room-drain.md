# Actual server coordinated drain

Connect the existing common drain barriers to the actual WebTransport room
control owner and actor. Preserve other recipients when an exact participant
closes after its Complete notice was admitted. Resource retirement is separate
from full-write evidence; retain frozen registry membership until every recipient
has actually retired and bound the final stage with its own deadline. Local
Complete full writes alone must not close connections before clients receive
their notices.

The actual server now calls the common relay's timed APIs with original capture
and admission observations. Only a successfully queued Complete permits that
recipient's retirement; retirement preserves the in-flight receipt without
crediting it. The shared terminal helper keeps other output queues and frozen
registry membership, guards stale leases, and releases the original room after
the final actual retirement. Complete full writes alone keep connections open.
The first accepted Ready starts a fixed deadline, including early readiness held
behind aggregate completion. Expiry scans retained rooms independently of live
resources. Queued controls precede joined I/O termination, preserving explicit
Leave and invalid-control cancellation. Relay admission/write status accessors
refuse after Stop.

Independent deferred fixtures add three actual server groups (13 total) and one
relay group (10 total). They cover 2/3/4/64 hosts, original capture floors, exact
outer receipts, early readiness, all-written connection retention, mixed and
all-retired cleanup, stale same-key replacements, failed queue admission,
cancellation and fixed deadlines. These tests have not been executed.

After both writers stopped, scoped Rust formatting and whitespace checks
completed. Four compile-only checks finished with exit 0: workspace/all-targets
with WebTransport, runtime/all-targets without defaults plus WebTransport,
and WASM libraries with browser and browser-audio. The WASM checks retain three
existing cadence dead-code warnings. Compilation does not prove delivery or
runtime behavior.

RoomPlay, browser bindings/Worker automatic final drain and native application
integration remain pending. No runtime or review/QA acceptance is claimed; the
full BMS player Goal remains active.
