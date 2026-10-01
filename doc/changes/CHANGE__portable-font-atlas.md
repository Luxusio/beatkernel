# Portable bounded font atlas preparation

The graphics library exposes font-byte parsing, fixed-scale glyph rasterization
and a bounded RGBA8 atlas cache through `font_atlas`. It reuses the already
resolved ab_glyph 0.2.32 package, now an optional direct graphics dependency,
and the existing raw texture resource format. Project-authored source stays
MIT; the dependency's Apache-2.0 license is retained in third-party notices.
No font asset is bundled.

Font data is limited to 32 MiB, scale to finite 1..128 pixels, each atlas extent
to 1..2048, and cached characters to 1..4096. Cache hits reuse glyph metrics
and stable UV rectangles; shelf packing adds transparent padding and rejects
overflow before changing admitted pixels, cache or placements. Glyph bounds
are relative to the baseline; advances are retained, outline-free whitespace
uses no atlas pixels, and the actual font's missing glyph is explicitly marked.
Raster coverage uses straight-alpha white RGBA for the existing tinted sprite
path. Preparation owns no file, GPU, input or audio handles.

Prepared fixtures build original minimal TrueType bytes with triangle outlines
and Latin/Hangul mappings to exercise real parsing and rasterization without
redistributing third-party fonts. They cover cache identity, coverage, padding,
missing/space behavior, capacity rejection and invalid configuration. Execution
remains deferred; source compilation is not visual or acoustic acceptance.

Known ceiling: this component is not yet selected by desktop configuration or
retained text atoms. That integration, font fallback, shaping, atlas updates and
actual native/WASM multilingual rendering remain required follow-up work.
