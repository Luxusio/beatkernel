# Prepared fonts in editable fields

The existing supplied-font option now reaches all editable desktop fields:
search, settings values/profile, display options, practice start/end and records
directory. Settings' visible nonselected values use it too. Text, caret, selected
ranges and IME underlines use the same actual font advances and field clip.
The visible borrowed window retains UTF-8 boundaries and its caret within the
1024-scalar drawing budget, including long zero-advance input.

New field characters are prepared at UI state boundaries. An all-cached batch
reuses its immutable atlas; a miss prepares a private candidate, preserving the
old cache on any returned error. Input values are capped at 4096 bytes each and
64 KiB per batch; the existing 4096-character atlas limit remains. Existing UVs
stay fixed, and the renderer updates the same custom texture at the same extent
without changing its identity, count or byte budget. Retained title packets stay
valid. Font changes repaint field nodes; identical bindings stay idle.

Admission failure preserves draft text and the previous atlas, displays an error
and switches current fields to bitmap text. Later admitted input restores the
supplied font. Renderer recovery uploads the current CPU atlas and rebinds its
new texture identity. No new crate or dependency is introduced.

Glyph preparation can copy the bounded atlas and upload pixels on a cache miss;
this is not an allocation-free UI claim. Audio/gameplay owners and retained paint
effects do no font parsing, rasterization or cache mutation. Shaping and fallback
fonts remain separate work; committed grapheme editing is specified in
[whole-grapheme editing](CHANGE__grapheme-editing.md), and borrowed field windows
in [whole-grapheme input windows](CHANGE__grapheme-windows.md).
Missing characters use the
supplied font's glyph zero. Labels and noneditable record metadata retain bitmap
text. Actual GPU pixels and keyboard/IME behavior remain unverified.

Regression fixtures are authored for atomic glyph admission, capacity failures,
metric windows, exact decoration geometry, texture-update admission, retained
field invalidation and actual desktop input/navigation preparation. Tests and
formal review/QA remain deferred by the user's instruction.

Rust 1.98.1 source checks passed on the first attempt for the workspace/all
targets, Windows GNU and macOS application/all targets, headless application/all
targets and WASM graphics library. Scoped formatting and diff checks passed.
The fixture bodies were compiled without execution. Existing macOS block
future-compatibility and WASM native-cadence warnings remain. These checks do
not establish native graphics/input acceptance, performance or an SDK-enabled
ASIO build.
