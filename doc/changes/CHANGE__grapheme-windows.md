# Whole-grapheme input windows

The bitmap and supplied-font editable-field projections borrow text windows
aligned to full-string Unicode extended grapheme boundaries. Scrolling no longer
starts with the combining mark or trailing member of a joined emoji. Native IME
cursor and selection byte endpoints remain unchanged; decorations still use the
renderer’s scalar columns or cached glyph advances. Complete composition is
preferred when its enclosing clusters fit the existing budgets.

Bitmap windows retain the requested scalar budget and omit an entire cluster
that cannot fit. An oversized caret cluster produces an empty caret window.
Supplied-font windows retain the 1024-scalar budget and explicitly reject a
caret cluster exceeding it. Whole admitted clusters may be wider than the field:
pixel clipping still applies, with caret pixels clamped when necessary. Cached
glyph metrics are borrowed without preparing glyphs or allocating windows.
Retained fields perform projection when their input geometry changes and reuse
the resulting geometry for ordinary unchanged redraws.

Five deferred groups cover bitmap budgets/full-string flag pairing and native
composition endpoints, actual synthetic prepared-font metrics/pixel cropping,
zero/tiny advances with the 1024-scalar limit, and complete cached-text validation
even for zero-width/offscreen windows. Literal slices and borrowed byte pointers
provide expectations; the tests do not use segmentation as an oracle.

After both writers returned terminal STOPPED, scoped formatting of six Rust paths
and whitespace inspection succeeded. Rust 1.98.1 compile-only checks completed
for workspace/all targets with WebTransport, headless/all targets with
WebTransport, WASM browser and WASM browser-audio libraries. The first check
compiled the new fixture bodies; none were executed. Existing WASM native-cadence
dead-code warnings remain. No generated bindings or player were run.

## Known ceiling

Text-window boundaries do not provide shaping, fallback fonts or complete
visibility of a cluster wider than its field. Independent glyph drawing still
uses its pixel clip and scalar count limits. Native IME/pixels and measured UI
performance remain unverified. Runtime tests and formal reviews/QA remain
user-deferred; the full player goal stays open.
