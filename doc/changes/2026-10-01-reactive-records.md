# Retained Records screen

Records now uses the shared retained UI node primitive for directory editing,
ten visible rows, paging, preview, opponent count, actions and messages.
The coordinator retains its view by Navigator instance, releases it on Back,
Watch branch exit or Closing, and uses event waiting after a successful idle
presentation. Metadata still uses the existing cancellable worker and requests
redraw on accepted completion. UI effects own geometry only.

Current-selection preview guards and pending action fences remain: all hit
regions disappear during metadata work, and Watch/Add require a valid preview.
The old renderer is a test-only adapter targeting the actual retained view.
Editable drafts with more than eight opponent paths remain visible, with
Clear All/Back available; existing Add/native admission owns that limit.
No native session, replay, catalog discovery or file format behavior changes.

Known ceiling: changed packets are allocated and concatenated, with complete
rectangle uploads. Fixed logical viewport and ten visible rows remain. Players
and Devices reactive migration, practice loops/live controls, browser adapters
and complete native/timing/GUI acceptance are still unfinished. No benchmark
or execution claim is made. Tests and all product/GUI/native/GPU/shader/device/
file/network execution and independent review/security/QA are user-deferred.
The full Goal and task remain open without acceptance PASS.

Source validation: Linux, Windows GNU and macOS app all-targets checks,
headless all-targets and WASM graphics library checks succeeded. Final affected
checks after retaining excessive opponent draft editing also succeeded. Scoped
Rust 2024 formatting and diff whitespace checks succeeded. Existing macOS
block future compatibility and WASM cadence warnings remain. Fixtures were
authored and compiled only, never executed.
