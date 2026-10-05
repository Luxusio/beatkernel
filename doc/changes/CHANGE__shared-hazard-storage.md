# Share prepared hazard data across judges

Immutable hazard markers and canonical initial-configuration bytes use shared
storage. Checkpoints retain that storage while copying mutable occupancy,
cursors, result buffers and interaction state. Canonical bytes, hash framing,
configuration compatibility and complete custom-state validation remain required.

Actual BMS local-member preparation constructs one source-aware pristine judge,
moves it to the first player and uses the existing validated checkpoint path for
additional players. Single-player preparation avoids a redundant checkpoint.
Prepared hazard-result and deadline-heap capacity must survive checkpoint copies.
Original player IDs, device routes and disjoint sound voices remain unchanged.

## Evidence and known ceiling

Both paired writers returned terminal stop reports. Five core and four actual
loader/local-runtime fixture groups are authored: shared marker/configuration
storage, independent state, value-compatible/atomic restore, retained capacities,
independent canonical hash framing and 1/2/64-player preparation with actual
shared-bank Mixer commands. Marker/configuration sharing uses Arc reference
counts during cold construction/checkpoint operations, not new per-note
synchronization. Mutable
deadline heap storage is rebuilt with the retained source capacity.

Scoped formatting and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets with WebTransport, no-default-features
WebTransport/all-targets, WASM browser/library and WASM browser-audio/library.
Host checks compile the fixture children; WASM library checks cover production
code without test children. Existing unused-code warnings remain. No assertions
have been executed. Custom interaction fault injection, runtime checks,
allocator measurements, platform acceptance and
formal review/QA remain deferred. Checkpoint validation still serializes state;
mutable buffers, interaction copies and routing structures can still allocate.
This does not establish allocation-free snapshots or measured latency gains.
The full BMS player Goal remains active; required browser QA is still outstanding.
