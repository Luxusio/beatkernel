# Admitted Leave cancels incoming room servicing

Successful Leave admission now releases incomplete-frame waiting and stops
inbound servicing while preserving genuine outgoing completion ownership.
A refused Leave keeps the existing receive deadline. Failed owners retain
their first error and cannot recover through cancellation.

The actual split-operation driver refuses incoming prefixes before decoding
without failing a healthy outgoing Leave. Its cancelled frame queries are idle,
and configuration cannot rearm waiting. The native IO driver services outgoing
bytes with its existing clocks and offsets, then skips reads while leaving.
The Worker discards settlement of an already pending read without new capture
observations or protocol/user callbacks. Actual full-write completion remains
required for leave_written; coordinated drain remains a separate protocol.

Native actor setup and coordinated-drain gates are cancelled after admitted
Leave. Outgoing Leave has one checked fixed bound using existing drain_timeout,
anchored to the original command observation. Existing pre/post IO observations
enforce inclusive expiry; original IO errors retain precedence. Real late write
history is retained without declaring timely success. Finish cleanup still uses
its existing finish_timeout.

## Known ceiling

Adapters still own bounded outgoing IO and cleanup. Cancellation cannot make a
blocked output complete, and does not synthesize an acknowledgement or successful
Leave receipt. Full browser room controller integration, broader BMS player
features and measured performance remain unfinished.

Two additional driver groups (nineteen total), two actual memory IO groups,
three actor groups (nine total) and two Worker adapter groups are authored for
later execution. Existing assertions are retained. Fixtures cover admitted and
refused Leave, unchanged partial decoder history, original output receipts,
discarded pending reads, inclusive pre/post deadlines, checked overflow and
original IO error precedence. The actor fixture also performs two actual
WouldBlock attempts, refuses deadline renewal and verifies expiry before a
third write. Worker fixtures use a fake facade and do not
independently establish Rust protocol behavior. Both writers returned terminal
stop reports before scoped Rust formatting. Four sequential final compile-only
checks exited zero: workspace/all-targets with WebTransport, no-default-features
WebTransport/all-targets, WASM browser/library and WASM browser-audio/library.
An earlier workspace check also exited zero before the author extended the
WouldBlock fixture; the final workspace check includes that extension. Existing
unused-code warnings remain. Host checks compile Rust fixtures; WASM checks
compile the actual binding-facing driver, not generated JS or fixture children.
JavaScript was read as text only. Tests, JS parsing/generated bindings,
browser/native transport timing and hardware acceptance, formal review, required
QA, verification and close remain deferred. The full Goal stays active.
