# Browser local peer presentation

The local browser game gains a passive peer display for each explicitly
admitted stable PlayerId, using the existing common SavedOpponentHud and exact
protocol progress validation. Admission occurs before activation and before
the target touch router is installed. One peer reserves 28 pixels in addition
to 14 pixels per saved opponent; the shared geometry supports the full eight
saved records plus one peer within a 140-pixel reservation.

Peer updates and display failure remain independent of saved comparisons and
other members. The reserved local compositor retains the peer's original row
offset when saved rows disappear, and failure indicators use their own
reserved rows. Display changes preserve judgment, score, capture, audio,
transport and touch geometry. Saved admission after touch setup is refused
because it would invalidate that geometry.

This is the Rust binding and common presentation layer needed for local
network competition. The current Window and Worker still refuse that mode:
actual participant connections, coordinated shared start, targeted progress
publication, final drain and page controls remain to be connected. No network
messages, peer scores or start evidence are fabricated by these APIs.

Execution, generated WASM bindings, browser/device/network acceptance and formal
review/QA remain deferred. Authored fixtures and Cargo checks are source and
compilation evidence only; they cannot establish playable behavior or measured
performance.

The deferred portable fixtures exercise the actual common HUD, borrowed local
compositor and shared-output StepLocalGameplay. They do not invoke generated
BrowserLocalGame JavaScript bindings. Browser-specific admission errors,
duplicate calls, touch locks and lifecycle behavior still need execution
through that actual WASM boundary during the later browser acceptance phase.

Two deferred fixture groups were added: the local saved-prefix fixture file
now has three groups and the common HUD fixture file has seven. Scoped Rust
formatting and whitespace checks completed. Workspace all-targets, headless
runtime all-targets, browser WASM library and browser-audio WASM library Cargo
checks each completed with exit code zero. The WASM checks retain three
existing platform cadence dead-code warnings. No fixture, browser binding,
Canvas indicator or network scenario was executed.
