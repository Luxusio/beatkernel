# Allocation-free numeric input roster verification

macOS local gameplay previously cloned the entire active HID device inventory
(including names), built a second candidate Vec, and rebuilt three HashSets and
a selected-ID Vec on every observation. Setup now retains that cold resolution
policy in a pure local-input module; steady-state verification uses a bounded
64-ID stack and native copied candidate lookups.
Solo observation now uses an exact pinned registry/runtime-ID membership query
instead of cloning the same inventory. It preserves the old membership rule
and HID loss checks; neither native query calls foreign code or exports records.

The HID adapter returns at most two registry matches as copied runtime-ID and
usability values. It allocates no device/name copies and exports no references
to callback-owned map entries across runloop pumping. The pure verifier compares
the full requested roster and retains original device/player identities.
Missing/changed members are deferred while invalid, ambiguous or aliased data
is checked, matching the cold resolver's refusal precedence. Existing HID loss
counters and native diagnostic strings remain unchanged.

Tests compare cold resolution and hot verification over 20,000 generated small
rosters, cover full-width identities and the 64-player boundary, and count actual
allocator calls across repeated 64-player happy/refusal paths. These tests do
not claim a measured native frame-time improvement or physical HID execution.
Independent code and security reviews passed, followed by scoped CLI QA:
runtime library 1,588 passed / 2 ignored, main 231 passed, macOS CLI 16 passed,
platform 204 passed / 1 ignored, and allocation integration 1 passed, all with
zero failures. Four pure policy fixtures executed, including 20,000 differential
cases and the 64/65-player boundary. Across 5,000 64-player happy/refusal calls,
the integration fixture measured alloc/realloc/dealloc counts of [0, 0, 0].
This count covers the pure verifier and copied numeric lookup, not error-string
boxing or actual IOHID callback processing.

Workspace all-target, WASM browser-library and latest macOS application source
checks exited 0. The macOS check used C SDK stubs, so actual HAL/HID/GUI execution
and native CPU frame-time improvement remain unverified.
The full player, independent input
collector and native device acceptance remain unfinished.
