# Deterministic replay and checkpoints

The user's 2026-09-30 clarification confirms that live play and replay must
share gameplay logic. Both route inputs and time advances through the same
JudgeEngine, evaluator and policy implementations; replay must not maintain a
second judging algorithm. Live acquisition/clock normalization/binding and
recorded normalized input are different input sources for that shared logic.
Seek and reverse reuse those same forward transitions after snapshot restore.
Audio hardware output and external rendering consume the resulting state and
events rather than deciding gameplay outcomes.

Phase 10 records normalized bound input together with unoffset song time and a global operation ordinal. Input ordering includes binding/report fanout. Explicit judge advances are recorded because timeout result timestamps depend on when the caller advances. The header contains a format version, chart/rules identities, options bytes, normalized clock domain, and seed. The application supplies durable chart/rule identities; they are not inferred from game names.

Checkpoints contain real interaction state, including held-button ownership, policy/resolver state, held input owners, effective time and scheduler deadlines. Optional object-safe snapshot cloning and canonical-byte methods default to unsupported, preserving existing custom implementations. An unsupported component fails explicitly. Snapshot implementations must deep-copy every mutable state and canonically encode every state that can affect future behavior; external side effects are outside the contract.

Restoration clones and validates all state before replacing an engine and preserves suppressed/consumed deadlines. Checkpoints are reusable and tied to the same compiled chart, profile and control routing. Seek and reverse inspection reconstruct forward from a checkpoint, replaying recorded operations at/before the target then advancing to that exact target; the forward judge never consumes decreasing song time. Recorded timeouts preserve their times; additional boundary misses use the target time. Boundary advances stay separate from the durable log, and boundary checkpoints include their target/effective time plus cursor. A boundary checkpoint can only service its exact target; using it for a later target would incorrectly substitute synthetic timeout times for recorded ones. Forking materializes an existing boundary advance into the new durable branch. Recorded result history and operation cursor are part of replay state. New input after a seek must explicitly fork the recording, removing its future records/checkpoints.

Stable hashes use versioned tagged little-endian encodings, length-prefixed sequences, sorted encodings of unordered ownership sets, full interaction/policy bytes and full result provenance. The fixed FNV-1a 64-bit algorithm is a deterministic divergence diagnostic, not a collision-resistant integrity/authentication primitive. Wall-clock telemetry, device queues and audible mixer state are excluded. Float samples use their exact IEEE bits.

Replay restores logical judging only. Audio output clocks and callback queues are not rewound by judge snapshots. Custom rule identities, deterministic behavior and snapshot support remain application obligations. Snapshot allocations run outside real-time callbacks. Verification and native/hardware execution remain deferred by the user's 2026-09-30 instruction; runnable fixtures accompany implementation without asserting PASS.

The live recorder captures accepted operations from RuntimeReport without running
a second judge. Successfully admitted bound inputs are recorded in report order,
including an accepted prefix before a fanout failure. An explicit successful
advance is recorded even if it emits no result. Unbound physical input, rejected
operations, CPU telemetry and output queue failures do not invent judge advances.
The normalized domain and song chronology are checked before appending a report.

Snapshot compatibility also includes canonical initial evaluator, resolver and
policy state captured at construction. Matching chart/profile/control IDs alone
cannot make differently configured rules compatible. Custom canonical encodings
must include rule implementation identity and immutable parameters as well as
mutable state, and support them from construction onward. Unsupported startup
state keeps logical live play available but disallows complete checkpoints.

`replay::codec` persists the ordered log in a bounded versioned envelope including
runtime version and optional calibration metadata. It embeds complete canonical
physical-input blobs with raw IEEE bits and validates domain/ordinal/chronology.
The log reconstructs checkpoints through supplied rules rather than serializing
trait objects. File IO belongs to examples/hosts, outside callbacks. Application
identity validation remains required before loading the intended chart/rules.
