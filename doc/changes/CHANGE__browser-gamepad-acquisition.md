# Bounded browser gamepad acquisition

Extend the physical input acquisition boundary with an optional Window
`GamepadInputOwner`. Preserve original browser sample timestamps, normalized
button/axis values and connection-scoped source identity. Explicit caller
polling has no internal timer or rendering. Shared source/sequence allocation,
bounded immutable snapshots, whole-poll preflight and fenced cleanup prevent
unbounded collection and accidental source reuse. No keyboard events or
synthetic disconnect releases are manufactured. Worker remains responsible
for control interpretation and gameplay.

The durable browser contract includes keyboard, touch, HID, gamepad and other
supported adapters as minimum main-thread acquisition scope. The
[W3C Gamepad specification](https://www.w3.org/TR/gamepad/) explains the Window
API and browser-normalized snapshots; its device indices can be reused and
product descriptions are not unique hardware identities.

Known ceiling: The new module is an acquisition component with no page/Worker
gameplay caller yet. Axis/button bindings, canonical forwarding, replay/capture
integration and actual browser/device/performance acceptance remain pending.
Polling cannot reconstruct intermediate transitions; missing browser lifecycle
evidence cannot prove physical reconnect identity. Source fixtures do not
establish hardware timing or successful execution.

Eight independently authored controlled-endpoint fixture groups cover sparse
full-width acquisition, timestamp/double fidelity, immutable snapshots before
allocator callbacks, reconnect identity, whole-poll refusal, exact capacities,
allocator exhaustion/regression, native/callback failures and cleanup/reentry.
Scoped whitespace checks emitted no diagnostics. No parser, assertions, tests,
Node, apps, browser/device/audio, generated bindings, formal review or QA was
executed. Unchanged Rust checks were not repeated for this JavaScript-only
slice. The full player Goal and Harness task remain active and unproven.
