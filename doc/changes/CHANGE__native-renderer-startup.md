# Owned native renderer preparation

Native player renderer initialization moves adapter/device/pipeline preparation
from the UI's synchronous wait into the existing owned preparation worker.
The UI still creates the native instance/window surface, transfers their owned
handles, and adopts the renderer only after nonblocking polling and actual join.
This extends [REQ__bms-player.md](../kernel/REQ__bms-player.md#owned-native-renderer-preparation);
WASM renderer initialization retains its existing async path.

While preparation is pending, the existing window shows a graphics-loading
title. Start, navigation, stale hits and drawing are fenced, while Escape and
OS lifecycle handling remain available. Catalog/font work may finish first,
but its result waits for renderer adoption. Adoption resizes using the current
window and uploads the current atlas before publishing renderer/font ownership.
Errors close explicitly without selecting another backend or presentation mode.

Occlusion retains a ready owned result until eligible. Suspension retires and
cancels its generation; delayed outcomes are discarded after join. Resume may
start fresh preparation only once retirement joins. Close cancels and joins
renderer preparation alongside the existing profile/catalog/game owners;
unexpected exit also releases the owned worker. No additional crate, executor
framework or parallel judging/rendering path is introduced.

## Source and deferred fixtures

`desktop.rs::initialize_renderer` creates the instance/surface on the window
owner and starts `NativeCatalog<PreparedRenderer>`. Its actual worker calls
the async `Renderer::new`; `collect_renderer` performs joined adoption.
`RendererStartup` tracks retirement, and pending state also fences the gap
before a real window has an adopted renderer. `about_to_wait` drains existing
owners, polls unfinished/eligible preparation and lets hidden ready ownership
wait for events. `suspended`, `request_close` and `exiting` retire/cancel/join;
the field drop order joins the transferred surface owner before releasing the
UI window reference. `native_catalog.rs` changes only ownership documentation;
its existing cancellation and join implementation is reused.

The producer and independent author both delivered actual terminal STOPPED
finals before scoped formatting or compile checks. The new
`desktop_renderer_startup_fixtures.rs` adds four groups using real owned worker
threads and channel gates. They cover pending UI/catalog fencing, eligible
error join, hidden retention, retired-generation discard/replacement and
close/Escape/Drop join. They create no fake successful renderer and preserve
all previous fixtures.

Scoped Rust formatting completed with exit 0 after both writers stopped.
The four authorized compile-only configurations were checked: workspace/all
targets with WebTransport, headless/all targets with WebTransport, WASM browser
library and WASM browser-audio library. The first workspace check (82331)
exited 101 because two new let-chain expressions required a newer edition than
the package's Rust 2021 edition. The producer restored nested conditions and
returned terminal STOPPED; scoped format and the affected workspace check
(28022) then exited 0. The other three checks (82112, 14153, 67763) exited 0
and did not cover the changed desktop conditions, so they were not repeated.
Existing WASM cadence dead-code warnings remain. These checks compile the new
fixture bodies; they do not execute them or prove native rendering acceptance.

## Known ceiling

Native instance/surface creation and adoption resize/texture upload remain UI
calls. Cooperative cancellation cannot interrupt a driver operation in progress.
Actual native surface/device/thread behavior, successful GPU adoption and
measured responsiveness remain deferred integration acceptance. Source-only
error/cancellation fixtures cannot prove successful rendering or full-player
completion. Assertions, app/device/browser runs and formal reviews/QA remain
deferred under the user's standing instruction.
