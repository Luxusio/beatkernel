# Coordinated room drain primitives

Extend the common room wire and progress owners with explicit client drain
readiness and a server completion notice after every host is genuinely ready.
Track original capture/admission observations and full-write receipts, keeping
early responses pending until their actual preceding write completes.

The common wire and client/relay state implementation is saved. DrainReady and
DrainComplete use additive tags 16/17 without changing existing frame bytes or
the frame cap. Explicit client readiness requires genuine local receipts; relay
completion requires all hosts' readiness and actual preceding aggregate writes.
Capture-time validation uses independent successful read and admission baselines.
Matching early notices remain behind their real full-write barriers.

Independent deferred fixtures add two wire groups (15 total), three relay groups
(9 total) and two client groups (8 total). The relay matrix composes the actual
client and relay for 2/3/4/64 hosts. It covers exact leases and final sequences,
capture floors, early notices, wrong receipts, legacy untimed refusal and Stop.
These fixture groups have not been executed.

After both source writers stopped, scoped Rust formatting and whitespace checks
completed. Four compile-only checks completed with exit 0: workspace/all-targets
with WebTransport, runtime/all-targets without default features plus WebTransport,
and WASM library checks for browser and browser-audio. WASM checks retain three
existing cadence dead-code warnings. Compilation is not runtime acceptance.

Existing progress completion remains separate from coordinated drain. Actual server
expected-close handling, RoomPlay/stream bindings and browser/native application
final drain remain required before automatic channel completion is enabled.
No tests, transport/browser/device/audio, generated bindings or formal review/QA
acceptance are claimed; the full player Goal remains active.
