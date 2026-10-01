# Explicit component clipping for selection text

Selection titles and artists now crop to their own 15-pixel line bands with
10-pixel horizontal row padding. A title without an artist uses the full
30-pixel row. Long text and supplied-font overhang cannot paint into adjacent
lines or rows. Existing original catalog hit bounds, filtered identities,
paging, retained repaint dependencies and renderer texture recovery remain.

The shared Scene primitive accepts an immutable checked ClipRect per sprite,
intersects it with the viewport and crops UVs in original sprite coordinates.
Positive clip extents and checked exclusive endpoints are required. There is
no mutable clip stack or GPU scissor state to leak to sibling components.
Legacy sprite, rect and bitmap glyph viewport behavior share the same crop
arithmetic and retain painter order, batching and sticky geometry-capacity
handling. No shader, real-time, native resource, dependency or crate changes.

Prepared-font draw_clipped preserves the existing two-pass cache/metric/position
preflight before geometry and stops at the intersected right bound. The new
bitmap text_clipped atom checks scale arithmetic before drawing and preserves
partial final glyphs. Both use a maximum 1024-scalar prefix. Atlas preparation
still occurs outside redraw, input acquisition and audio callbacks.

Prepared regression fixtures cover four-edge UV cropping, signed and overflow
clip bounds, invalid-UV atomicity, invisible clips, retained append and sibling
clip isolation, sticky capacity, real synthetic-font crop/preflight behavior,
bitmap partial glyphs and actual Selection long-text/overhang bands with stable
hits and repaint counts. Tests and actual GUI/GPU execution remain deferred;
source compilation does not prove rendered appearance or physical performance.

Known ceiling: clipping can cut overhanging glyphs. Ellipsis, wrapping,
shaping/fallback and broader widget-font integration remain pending. Existing
f32 geometry precision and fixed scene/atlas capacities remain in effect.

Source checks completed successfully with Rust 1.98.1: host workspace/all-targets,
Windows GNU and macOS application/all-targets, headless application/all-targets,
and the WASM graphics library, all with locked dependencies. Scoped formatting
and diff checks also succeeded. Fixtures were compiled where applicable only;
no tests, native display or formal acceptance checks were executed.
