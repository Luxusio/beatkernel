# Reusable playfield visibility queries

The live scene reuses chart-local visibility indices instead of allocating a new note-reference vector each frame. One lazily populated scratch vector serves up to four displayed playfields and retains its allocation across scene clear, forward frames, backward seeks and chart replacement. Its first query with candidate heads reserves the complete 2,049-entry capacity, avoiding repeated growth as notes enter view; static UI scenes acquire no visibility backing storage. It never retains borrowed chart references. Indexed queries share the existing endpoint-tree traversal and exact inclusive head/body window semantics with the original public APIs.

Visibility overflow clears the scratch result and rejects the frame before admitted playfield/batch insertion. Lane mapping validation remains before insertion. GPU note cache membership, immutable instance reuse, local timestamp epochs and seek/geometry invalidation stay unchanged; no per-note signals or chart-wide per-frame scans are introduced.

## Known ceiling

Successful visibility queries reuse sufficiently warmed backing storage. Cache membership/epoch/geometry rebuilds, other scene geometry and error strings may still allocate; this is not a whole-frame zero-allocation or measured performance claim. The existing 2,048-visible-note and four-playfield admission bounds remain. Native GPU execution and performance measurements are deferred.

Four new fixture groups cover a linear overlap oracle against the shared indexed/reference traversal, negative/extreme timestamp windows, exact/overflow budgets, retained pointer/capacity across forward/backward/empty/replaced charts, indexed/reference GPU geometry and cache identity, and the actual four-slot Scene path with growing membership and atomic admission failures. An existing pure Scene fixture gained its missing WAV definition. Tests are authored and compiled for later execution.

Source checks completed with exit code zero for Linux workspace/all targets, Windows GNU/all targets, macOS/all targets, graphics-disabled/all targets and WASM graphics/library. Scoped formatting and whitespace checks completed. Native all-target checks compile tests without executing them; existing macOS `block` and WASM cadence warnings remain. Native/shader/benchmark and formal acceptance remain pending, and the full player task remains open.
