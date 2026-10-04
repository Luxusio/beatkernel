# Native IME editing-area positioning

The desktop host submits the focused input field's editing area through winit's
native IME API after composing that field on the current screen. The rectangle
comes from the actual retained hit geometry, avoiding duplicate layouts and
another caret-metric calculation. The renderer's shared contained viewport maps
it to window-client physical pixels, rounded outward and clipped to the surface.
Physical coordinates are supplied directly without applying DPI twice.

Unchanged submissions reuse a window/target/area cache. Focus or screen changes,
unavailable UI, stale or absent geometry, window replacement and resize revoke
that cache. Redraw after resize or scale change supplies the current area.
Zero-sized surfaces and unrepresentable native coordinates yield no submission.
This API advises the OS about the editing region; it cannot choose or acknowledge
the popup's actual position.

Three deferred fixture groups use actual composed Desktop hit rectangles for
every current field, including a paged settings row. They cover literal identity,
wide/tall bars, odd fractional extents and tiny surfaces; zero dimensions and
signed native-origin/end overflow; and missing/stale target, filter, pending
operation and lifecycle refusal. They open no native window and do not establish
cache suppression or OS submission/placement by simulation.

After both writers returned terminal STOPPED, scoped formatting and whitespace
inspection succeeded. Rust 1.98.1 compile-only checks completed for the workspace
and all targets with WebTransport, headless/all targets with WebTransport, WASM
browser and WASM browser-audio libraries. New fixture bodies compiled without
execution. Existing three WASM native-cadence warnings remain. No app, generated
bindings or native popup was run.

## Known ceiling

[winit 0.30.13](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.set_ime_cursor_area)
documents X11 support for position only and no support for iOS, Android, Web or
Orbital. Candidate layout depends on the native IME/backend. This implementation
uses the whole field's area; exact glyph-caret placement is separate work.
Shaping/fallback and real native popup/display/input acceptance remain pending.
Runtime tests and formal review/QA remain user-deferred; the player goal is open.
