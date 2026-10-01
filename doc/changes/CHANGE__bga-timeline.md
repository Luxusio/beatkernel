# BMS image selections on the original song timeline

The adapter previously discarded BMP resources and channels04/06/07. It now
retains typed image references, opaque resource paths and independent base,
poor and layer events. Image decoding and rendering remain pending. BMP00 is
the initial poor selection; channel06 updates its reference without prescribing
a miss-overlay display policy. Zero row tokens are rests. Undefined nonzero
references stay selected for future blank-resource rendering. Definitions and
markers obey existing seeded branch admission and duplicate policies.

An independent bounded visual grid includes gameplay denominators and projects
visual markers through the checked core compiler's BPM/STOP semantics. Visual
subdivisions do not change the gameplay resolution, object identities or BGM
times. Existing object metadata still includes gameplay source line numbers,
so moving gameplay lines can change replay identity; this work does not remove
that pre-existing behavior. Gameplay objects never include visual-only markers.

PlayerChart prepares a portable BgaTimeline before session publication. Three
channel indexes answer at-or-before original-song timestamps using binary
search, without allocation, file IO, relative elapsed time or a forward cursor.
Pauses reuse the same state; backwards seek and fresh practice restart project
the exact earlier prefix. Live/local/replay published charts expose the same
bga_state query. Negative preroll retains the initial poor selection but has
no timed base/layer selection. Resource definitions do not open any files.

Authored parser/compiler and pure projection fixtures cover definitions,
duplicates, zero rests, undefined resources, seeded conditionals, bounds,
visual-only resolution, irregular measures, BPM/STOP and equal-time ordering.
A real Runtime input/capture/codec/reconstruct/ReplayVisual/publisher fixture
checks original-song timing across STOP, pause and a fresh practice transport.
Fixtures are prepared for later execution. Source compilation is the only
validation performed in this phase; no native GPU/device acceptance or measured
performance claim. Static raster decoding/loading, GPU upload/display,
video/crop/opacity and poor-overlay policy remain future work.

Original BMP00/04/06 definitions: [author's BMS format](https://bm98.yaneu.com/bm98/bmsformat.html).
Layer07 and implementation conventions: [hitkey command memo](https://hitkey.nekokan.dyndns.info/cmds.htm).

Source check evidence: host workspace/all-targets, Windows GNU app/all-targets,
macOS app/all-targets, app no-default/all-targets and WASM graphics/lib all
finished with exit0. Scoped Rust formatting and whitespace checks also exit0.
Tests, native rendering/devices and formal review/QA remain deferred.
