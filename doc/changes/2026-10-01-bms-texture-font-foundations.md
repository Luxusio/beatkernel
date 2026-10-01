# Texture and glyph primitives

The graphics foundation now validates raw RGBA8 resources, uploads/removes
custom textures with unique typed IDs and finite count/byte admission, and
renders clipped sprite UV rectangles. Solid, glyph and image geometry use
contiguous texture batches in painter order, with straight-alpha blending and
nearest sampling. The existing UI text now uses a built-in bitmap atlas and one
quad per glyph instead of a quad per lit glyph pixel. White/font resources
cannot be removed; stale or foreign custom IDs fail before frame acquisition.
These portable primitives prepare later font/image components without adding a
crate or changing native input/audio ownership. Multilingual font shaping,
image-file decoding and the browser/native runtime acceptance remain pending.
Source and fixture compilation plus formatting are permitted; tests, shaders,
actual GPU/browser/native execution and formal reviews/QA remain deferred by
the user's instruction. The [player contract](../kernel/REQ__bms-player.md)
records resource limits, draw batching and unresolved acceptance.
