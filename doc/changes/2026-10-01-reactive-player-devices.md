# Retained player and device setup UI

Players and Devices now use the shared retained node primitive. Visible rows,
selection, summary, page/action buttons, pending metadata and messages have
separate dependencies. The coordinator retains Players beneath its device
child, restores it on Back and releases both scopes on Closing. Current-frame
hit projection avoids stale cached hover after scene restoration. Idle menus
wait for events with the existing surface retry and metadata redraw rules.

Solo automatic input, stable positive player IDs, the 64-player roster bound,
distinct keyboard assignment and native session ownership remain. Device rows
marked unselectable remain visible without click admission. External Use and
assignment validation still own admission; UI effects do no native work.

Known ceiling: fixed logical viewport, ten visible rows, packet allocation and
full scene concatenation/rectangle uploads remain. All currently implemented
menu routes now use retained nodes; the full widget host, browser adapters,
practice loops/live controls and complete native/timing/GUI acceptance are
still unfinished. Tests/product/GUI/native/GPU/shader/device/file/network/bench
and independent review/security/QA remain user-deferred. No acceptance PASS or
Goal completion is claimed.

Source validation: Linux, Windows GNU and macOS app all-targets, headless
all-targets and WASM graphics library Cargo checks succeeded. Scoped Rust
2024 formatting and diff whitespace checks succeeded. Existing macOS block
future compatibility and WASM cadence warnings remain. Six view fixture groups
and one coordinator lifecycle group were authored and compiled, never executed.
