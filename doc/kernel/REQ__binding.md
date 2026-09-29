# Physical-to-game binding

Phase 3 of [the full specification](../../plan.md) maps physical input to game-defined controls and stream channels. The core keeps OS key codes and game-specific rules outside this mapping.

## Public values and ownership

`GameControlId(u32)` names a caller-defined logical control. `DeviceSelector` is `Any` or `Exact(DeviceId)`. A `Binding` contains the selector, complete `PhysicalControlId`, and logical destination.

`GameInputEvent` owns `game_control` and an unchanged `physical: PhysicalInputEvent`. Yielded events can outlive both the input and map. Source identity, physical identity, timestamp/domain, acquisition sequence, native provenance, and original clock point remain intact.

Button, axis, touch, pointer, and pose inputs retain their full typed data. Touch contacts and phases, pressure, axis/pointer modes, and unnormalized pose coordinates/quaternions are never flattened into button actions. Floating-point samples keep their original bit patterns.

Raw HID reports and custom byte payloads have no semantic control identity and produce no binding outputs. Callers interpret them with a `DeviceAdapter` before mapping the resulting typed controls.

## Matching and configuration

- Match the full physical identity, including HID page, native backend, or vendor namespace, and the runtime source ID.
- If any matching `Exact(source)` rule exists for this physical control, use only matching Exact rules. Otherwise use every matching Any rule. An exact rule for another device/control/namespace cannot suppress fallback.
- Multiple logical destinations fan out in insertion order. The same selector/control/destination triple is a duplicate; a different destination is valid.
- `BindingMap::new()` and `Default` are empty. `from_bindings` preserves validated input order and returns `BindingError::Duplicate(binding)` if a duplicate is encountered; no partial map escapes.
- `add` rejects a duplicate without changing stored bindings. `bindings()` exposes an immutable ordered slice.
- `remove(&Binding)` preserves remaining order, returns false for an absent rule, and allows readding at the end. Removing the last applicable Exact rule restores Any fallback.
- `map(&PhysicalInputEvent)` yields owned events from immutable current configuration. Unknown semantic controls, empty maps, raw reports, and custom payloads yield an empty iterator.

Mapping does not retain held state. Editing configuration during a stream can change destinations of later samples. Consumers coordinate such edits with their gameplay state; this phase does not synthesize releases or transfer held controls.

## Allocation and complexity

Mapping uses at most two linear passes and fixed-size clones of supported semantic variants, with no heap allocation or mutation. Raw/custom variants are excluded before cloning. Configuration edits can allocate, lookup/removal are linear, and construction through validated adds is quadratic. This is not a measured latency or audio-callback suitability claim.

## Verification

`crates/beatkernel/tests/binding.rs` must verify two virtual keyboards with the same HID A mapping to different exact controls, Any/Exact precedence including unrelated rules, ordered fanout and editing, duplicate atomicity, namespace separation, all typed payload/provenance bits, owned lifetimes, and virtual FIFO-to-binding integration.

Run `cargo test -p beatkernel --test binding --locked` and `cargo run -p beatkernel --example binding --locked`, followed by workspace debug/release tests, strict Clippy, formatting, and documentation checks in [the manifest](../harness/manifest.yaml).

The example is virtual and demonstrates two keyboard identities and a retained touch channel. Native acquisition, chart/judge/audio/replay, persistence, text input, and held-state coordination remain later work under the full specification.
