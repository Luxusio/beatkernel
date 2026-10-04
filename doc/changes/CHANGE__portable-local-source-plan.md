# Portable resolved local input ownership

Add an owned resolved source plan using stable PlayerIds and canonical DeviceIds.
It has no native settings host, path or browser product identity. Its bounded
four-word numeric codec preserves full-width sources and explicit automatic
versus exact selectors. Solo can remain automatic; multiple members require
distinct exact sources. Zero PlayerId is refused, while DeviceId zero remains
valid in the canonical model.

Share setup validation with the real RuntimeGroup and native InputPlan resolver.
Validate all native draft identities before lookup; actual lookup failures retain
their external prefix, with no simulated rollback. Existing exact binding
selectors, telemetry bounds and voice ownership remain enforced. Input routing,
judge execution and audio callbacks gain no new validation or representation.

Five independently authored deferred fixture groups cover literal wire
roundtrips and owned snapshots, malformed selectors/counts/identities through
64 members, native preflight and lookup prefixes, actual RuntimeGroup admission,
and actual three/four-member routing with independent judges on one original
transport and command queue. Fixture source and compilation do not establish
that assertions or runtime behavior passed. Tests, device/browser/audio runs,
generated bindings, formal reviews and QA remain unexecuted.

Scoped Rust formatting and staged whitespace inspection completed cleanly.
Four compile-only configurations completed with exit zero: workspace all
targets, headless app all targets, wasm32 browser library and wasm32 audio
library. Existing three platform cadence dead-code warnings remain on WASM.
Initial host checks failed because the coordinator used relative compiler
wrapper paths; both were rerun successfully with existing absolute paths.

The browser still needs its shared nonblocking group owner, actual device
assignment and multi-field presentation. This foundation does not establish
browser local-play or hardware/runtime acceptance. The full player Goal remains
active and the required ordered reviews and browser/CLI/desktop QA remain pending.
