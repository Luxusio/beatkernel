# IME composition ranges in retained text fields

Search, native settings values and profile-path fields now underline the active
IME composition and highlight its selected portion. LineEditor retains absolute
UTF-8 byte ranges separately from committed text. Its borrowed visible projection
converts them to clipped scalar columns, keeps the caret position in view and
shows the whole composition when it fits. A missing native cursor range hides
the caret; a collapsed range still displays it. The existing shared text-field renderer paints
the decorations only while focused. No extra per-frame signal or allocation is
introduced for range projection.

The desktop already passes preview editors to retained Selection and Settings
views, so the event routing remains unchanged. A selection-end-only change now
participates in editor equality and repaints just the relevant field. Committing,
disabling IME or changing fields clears preview decorations without leaking text
into another draft. Invalid edits preserve text, cursor and composition metadata.

Prepared fixtures cover multibyte ranges, clipped/narrow/empty windows, metadata
clearing and rejected edits, literal scene geometry, retained field-only repaint,
idle updates and actual desktop event-to-scene commit/cancellation behavior.
Tests and native visual acceptance remain deferred.

## Compilation evidence

Rust 1.98.1 source checks passed for the workspace/all targets, Windows GNU and
macOS application/all targets, headless application/all targets, and WASM
graphics library. Fixtures were compiled, not executed. The first native checks
found a desktop fixture accessing library-private Scene geometry. Geometry
assertions now stay in library fixtures; the desktop fixture uses public preview
metadata and actual retained draw calls without widening the Scene API.

The installed winit 0.30.13 Preedit contract specifies that a missing cursor
range hides the caret. After that correction, all four affected UI configurations
passed another source check; the unaffected headless check had already passed.
Existing macOS block future-compatibility and WASM native-cadence warnings remain.
No SDK-enabled ASIO build, tests, native execution, review or QA was performed.

## Known ceiling

Input fields still use the existing bitmap glyph fallback. Multilingual shaping,
general committed-text selection, clipboard operations and OS candidate-window
positioning are separate work. This change covers the existing IME-enabled
search/settings/profile fields and does not enable IME in other dialogs.
Native IME event ordering still lacks a generation identifier; existing active
target and enable/disable guards remain, without an absolute stale-event claim.
No GPU/window/device execution or physical platform acceptance is established.
