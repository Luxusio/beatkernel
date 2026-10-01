# Retained fine-grained Settings screen

The actual desktop Settings route now owns one retained Floem reactive view per
Navigator instance. Ten row slots subscribe to individual field signals, with
the active editor/cursor observed only by the selected row. Separate profile,
focus, hint, page, message/error and control dependencies avoid rebuilding
unrelated geometry. The view borrows the draft, compares before cloning changed
values, and keeps its nodes/signals through profile loads, reordered fields and
Add Binding. NativeSettings itself still owns validation and applied options.

Settings bindings survive Display, Practice, Records, Players and Devices
children and resume with their original instance on Back. Branch exit/Closing
releases their scope; native game/metadata owners stay independent. Existing
pending-operation input/navigation guards remain. Pending view state removes
all control/field/profile hit regions. Buttons share one authoritative layout
for painting and hover projection. A centralized hit invalidation boundary
forces scene restoration even when a no-op selection left every signal equal.

Idle Settings joins Selection and Practice on the event-driven redraw path.
The metadata owner queues redraw when it accepts a completion, before its loop
can switch to Wait; profile success/error and Save therefore require no later
user input to update. Pending tasks keep polling. Existing surface timeout/
outdated/lost retry cadence remains. The obsolete immediate Settings drawing
function is deleted; newly authored UI fixtures target the retained view.
No new dependency, crate, batch scheduler, native clock or note binding was added.

Known ceiling: 128 field signals remain allocated for the existing model bound,
while visible rows are limited to ten. Updates compare up to the existing
64 KiB field values, and changed nodes trigger full packet concatenation and
rectangle buffer upload. Multi-field profile replacement can repaint intermediate
memo outputs synchronously before the final composition; there is no batching
scheduler. Display, Records, Players and Devices still need migration, and the
full widget host remains separate. The fixed 960x720 logical viewport and
existing glyph/presentation limits remain.

Four view fixture groups cover cursor/status/focus selectivity, page/reorder/add
identity, pending/invalid-frame admission, forced composition and scope disposal.
One root desktop group covers child retention and hit-cache restoration on a
no-op selection. Fixtures were authored and compiled only. Actual tests,
GUI/native/GPU/shader/device/file/network product execution, benchmarks and
independent formal review/security/QA remain user-deferred; no acceptance PASS
or full Goal completion is claimed.

Rust 1.98.1 app all-target source checks succeeded on Linux, Windows GNU and
macOS; headless all-target and WASM graphics-library checks succeeded. Scoped
rustfmt and git diff --check succeeded. Existing macOS block0.1.6 future-
incompatibility and WASM cadence dead-code warnings remain.
