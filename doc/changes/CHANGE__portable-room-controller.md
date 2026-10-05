# Shared room controller and native compatibility

Room competition policy moves into the unconditional `RoomCompetition` module
with explicit network, UI and runtime hosts. Native construction and physical
start-agreement adaptation remain in the compatibility module. Existing native
calls retain their default hosts; portable callers use the same lobby, progress,
start and cleanup policy with injected implementations. Generic startup returns
the original typed service refusal while keeping retained diagnostics separate.

## Known ceiling

Browser adapter integration, worker IO, actual network/device timing and
cross-platform acceptance remain unfinished or unverified. Retained result
presentation remains cold controller policy. Existing native regressions remain
host-only fixtures. Six new pure fixture groups are authored but unexecuted.
After both writers stopped, scoped formatting and four sequential compile-only
configurations completed with exit zero: workspace/all-targets WebTransport,
no-default-features WebTransport/all-targets, WASM browser/library and WASM
browser-audio/library. The initial host check exposed unused relocated test
imports, removed by the source owner before the successful repeat. The initial
WASM browser check exposed an obsolete native-only gate on the pure joined-prefix
HUD operation; the owner removed that gate without changing its validation, and
the affected WASM check succeeded on repeat. Remaining warnings concern existing
unused code. Both WASM configurations compile the actual shared controller;
library checks do not compile its fixtures. No runtime, benchmark, formal review
or QA claim is made by this module extraction.
