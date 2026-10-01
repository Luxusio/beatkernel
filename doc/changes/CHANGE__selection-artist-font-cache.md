# Selection artists and shared font glyph placements

The actual retained selection rows now display nonempty artist metadata below
the title, so artist search results expose the matched metadata. Empty artists
keep the previous title placement. Original catalog control identities, row hit
bounds, paging and projection-driven repaint are preserved; the new line belongs
to the existing row node and adds no reactive subscriptions.

Bitmap artists use a smaller line. With `--title-font`, titles and artists share
the renderer's fixed-scale immutable font texture. Startup prepares the first
1024 scalars of both fields before creating the window/native owner; existing
32 MiB font, 4096 character and 1024-square atlas bounds still apply. Renderer
recovery rebinds the existing resource path with a new texture identity.

Portable FontAtlas now shares metrics and UV placements by actual font glyph
identity for non-whitespace aliases, including glyph zero. Aliases occupy their
own character cache slots but do not consume another raster placement.
Whitespace remains geometry-free and cannot populate the drawable identity
cache. Returned errors preserve both caches, existing pixels and shelf state.

Prepared regression fixtures use the original synthetic TrueType font and real
retained composition: exact-fit alias reuse, missing-glyph reuse, whitespace
ordering, atomic capacity failure, bitmap/prepared artist placement, filtered
row identities, renderer replacement, unchanged repaint counts and uncached
artist rejection. Execution remains deferred; source compilation alone does not
establish native visual appearance or performance.

Known ceiling: character capacity remains 4096 even for glyph aliases. Arbitrary
supplied fonts can overhang metrics; per-row text clipping, shaping/fallback,
font integration into other widgets and native visual acceptance remain pending.

Source checks with Rust 1.98.1 succeeded: workspace/all-targets on the host,
application/all-targets on Windows GNU and macOS, application/all-targets without
default features, and the WASM graphics library. Initial compile checks found
and corrected Rust 2021 syntax and binary/library test-fixture linkage issues;
the shared synthetic font generator now compiles only in test modules. Scoped
formatting and diff checks succeeded. Fixtures were compiled, not executed;
GUI/GPU/native device and formal acceptance checks remain deferred.
