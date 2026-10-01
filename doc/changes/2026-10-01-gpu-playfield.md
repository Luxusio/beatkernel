# Retained GPU playfield trial

The graphical player now inserts a separate instanced GPU note layer between
lane backgrounds and the judgment line. It retains visible note geometry
across frames and uploads it again only when membership, lane bounds, scroll
lookahead or a local precision epoch changes. Each displayed player receives a
32-byte uniform with actual presentation-time drift and clip bounds. Integer
timestamp subtraction precedes floating conversion; bounded local coordinates
keep long charts and spanning holds from losing precision through absolute
f32 time. Reverse time invalidates the cache, and forward drift beyond one
quarter of lookahead rebases it. Dense overlap above 2048 notes now reports an
error rather than silently truncating the displayed notes.

MVVM-like presentation state and the separate GPU playfield are the documented
architecture. Menu toolkit migration and actual navigator integration remain
unfinished; this change does not establish reactive menus in the desktop host.

Known ceiling: visible selection still allocates and compares note data on CPU
each frame. Dense streams can invalidate the instance cache every frame; GPU
fill and geometry still grow with displayed content. Four fields and 2048
notes per field are bounded admission limits, not performance guarantees.

Linux, Windows GNU, macOS and headless app all-target source checks and WASM
graphics-library source checks succeeded using Rust 1.98.1. Cache/seek/long-time,
painter-order and dense-admission fixtures were authored and compiled only.
Tests, shader validation/execution, native GPU rendering, benchmarks, formal
independent review and QA remain deferred by the user. Source compilation does
not demonstrate rendering correctness or a measured speed improvement.
