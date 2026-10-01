# Prepared fonts for retained catalog titles

Desktop `--title-font PATH` prepares a caller-provided TrueType/OpenType font
before window startup and draws catalog titles through the existing ordered
sprite renderer. Reads are bounded to 32 MiB plus one rejection byte. The atlas
uses 14 pixels, 1024 by 1024 texels and at most 4096 cached characters; each
single-line title considers at most its first 1024 Unicode scalars. Preparation
errors are explicit. No font asset is bundled.

Title rows retain immutable glyph metrics and UVs. Painting uses actual advances
and baseline bounds without rasterization, glyph preparation or native I/O.
Used uncached glyphs are rejected before text geometry writes. Retained paint
errors prevent snapshot admission. Full renderer recovery uploads a fresh
texture and rebuilds Selection; surface-only recovery keeps the same resources.
The option does not enter native playback, replay or saved profile arguments.

Controls and other screens still use bitmap text. Shaping, kerning, automatic
font discovery, fallback and broader text-widget support remain future work.
This change does not establish actual native visual or GPU acceptance.

Self-authored font fixtures cover cached text placement and whitespace advances,
uncached-text rejection, viewport/scalar-prefix limits, retained title textures, unchanged updates, filtered
rows, a replacement texture identity and retained paint-error recovery. Desktop
fixtures cover option ownership/rejection and retained-view invalidation while
preserving search. These fixtures are prepared and compiled for later execution;
tests, GUI/device execution and formal review/QA remain deferred by the user.

Locked source compilation succeeded for workspace all-targets on Linux, app
all-targets on Windows GNU and macOS, app all-targets without default features,
and the WASM graphics library. Scoped rustfmt and diff checks also succeeded.
These checks establish compilation, not runtime fixture or visual correctness.
