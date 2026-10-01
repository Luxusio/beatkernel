# Shared retained UI component base

Selection, Practice and Settings now use the same RetainedNodes primitive as
Display. Per-view Packet/paint/effect storage/dirty/composition implementations
are removed. Each view still owns its Floem scope and signal dependency graph;
button/row semantics, immutable painter order, stable hit IDs and Navigator
instance lifetime remain. Scope disposal stays explicit and main-thread Rc
storage remains non-Send. There is no new crate, dependency, scheduler or state
engine.

RetainedNodes validates initial geometry packet errors before constructors
return. Display now calls that same validation. Later composition failures remain
explicit, retain dirty state and cannot reach partial renderer submission.
Existing dependency, page, admission, hit-order and disposal fixtures now target
the shared implementation. Test instrumentation remains cfg(test) only.

Known ceiling: changed nodes still allocate small geometry packets, concatenate
visible packets and upload the whole rectangle buffer. Records, Players and
Devices still need reactive migration, while full widget-host/browser/native
control and acceptance work remain. No actual benchmark, GUI or acoustic proof
is claimed. All tests/product/GUI/native/GPU/shader/device/file/network execution
and independent formal review/security/QA remain user-deferred. The full Goal
and Harness task remain open without acceptance PASS.

Source validation: Linux host, Windows GNU and macOS app all-targets, headless
all-targets, and WASM graphics library Cargo checks succeeded. Scoped Rust 2024
formatting and diff whitespace checks succeeded. Existing macOS block future
compatibility and WASM cadence warnings remain. Fixtures were compiled only;
no tests or application were executed.
