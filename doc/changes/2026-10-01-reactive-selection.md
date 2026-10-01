# Retained reactive Selection screen

The desktop's Selection screen now creates one retained node tree per Navigator
instance and uses floem_reactive 0.2.0 for signal dependency tracking. Row-slot
and button memos suppress unchanged output. Selection, hover, press, error and
backend-pending state are separate signals; status updates do not repaint rows.
Static labels/diagnostics are prepared once. Opening Settings retains the
Selection scope, Back restores it, and leaving the branch disposes its signals
and effects. Reactive geometry owns no gameplay clocks, native handles or I/O.

Scene supports immutable component geometry packets with checked viewport and
capacity admission and preserved painter order. The desktop composes those
packets only when a dependency changed or another route overwrote the scene.
Unchanged redraws keep the composition. The renderer uploads rectangle instances
only for a different scene identity/geometry epoch; counter exhaustion rotates
identity rather than aliasing an old upload. Idle Selection uses event-loop Wait
and requests drawing on input/window events. Resizing invalidates composition
and hit regions. Gameplay cadence and note time uniforms stay independent.

Only the standalone MIT reactive engine is linked. Its sole dependency is the
already-present smallvec; Cargo.lock adds one package. The published Floem 0.2.0
host uses wgpu22 and forked winit0.29.5, so adopting that entire host would require
separate integration with this app's wgpu27/winit0.30.13. The engine package
omits a LICENSE file; the MIT text retained in third-party notices comes from
the published parent Floem 0.2.0 package's LICENSE. No default GPL dependency is
introduced.

Known ceiling: Settings, Records, Display, Players and Devices still use the
previous drawing path. Changed Selection nodes trigger concatenation of all
visible packets and a full rectangle buffer upload, not GPU subrange writes.
Effects currently construct small Scene packets when their output changes; this
is bounded to fixed visible nodes but is not an allocation/performance benchmark.
Selection requires the existing 960x720 logical viewport. Catalog refresh/search
and a full widget toolkit host migration are separate work.

Pure dependency-selectivity, scope-disposal, page/hit identity, retained packet
order/capacity/identity-wrap and desktop Back/restore fixtures were authored and
compiled only. Actual tests, GUI/native/device/GPU execution, shader validation,
performance measurements and independent formal review/security/QA remain
user-deferred. The full player Goal and Harness task remain open; no acceptance
PASS or completion is claimed.

Event-driven Selection retries drawing at the configured cadence after transient
surface acquisition timeout, outdated configuration or lost-surface recreation.
It enters idle Wait only after a presented frame (or a zero-size suspended
surface), so a recovered surface cannot wait indefinitely for unrelated input.

Rust 1.98.1 final source compilation succeeded for app all-targets on Linux,
Windows GNU and macOS, and for the WASM graphics library after the surface-retry
refinement. Headless all-target compilation also succeeded before that graphics
only refinement. Scoped rustfmt checks and git diff --check succeeded. Existing
macOS block0.1.6 future-incompatibility and WASM cadence warnings remain.
