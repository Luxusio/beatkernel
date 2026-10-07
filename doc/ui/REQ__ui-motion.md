# Retained UI translation without Virtual DOM

Keep declaration structure and cached static geometry when moving a UI surface.
The first motion primitive translates all ordinary scene geometry together in
logical pixels. Apply translation through the rectangle renderer's viewport
uniform, preserving texture coordinates, painter order and geometry upload
identity. Translation alone must not repaint packets or change the geometry
epoch. Timed note/playfield geometry remains on its separate rendering path.

Pointer hit projection must use the inverse of the same translation after
physical-to-logical viewport projection, and reject bars/nonfinite points and
points outside the visible logical viewport. Existing untranslated hit bounds
and keyboard/navigation ownership remain valid. Invalid transform input must
reject before mutating the admitted state.

Provide a pure translation sampler driven by caller-supplied elapsed duration.
It owns no timer, thread, transport clock or IO. For a positive duration it
must return the exact starting offset at elapsed zero and the exact ending
offset at completion, clamping after completion. Zero duration returns the
destination immediately, including at elapsed zero. Intermediate displacement
truncates toward zero, rounding movement toward the starting offset: moving
from (0,0) to (-3,3) at one quarter of the duration yields (0,0). Sampling must
handle `Duration::MAX` and extreme coordinate differences without overflow.
Admit offsets only within ±2^24
logical pixels so integer offsets have an exact f32 uniform representation;
reject larger offsets atomically. Initial integer-pixel translation
matches the existing bitmap UI; fractional transforms and easing are future
extensions rather than claimed existing behavior.

The live scene owns translation separately from local cached geometry. Clearing
a scene resets translation, preventing a moved screen from affecting the next
route. A presentation owner applies the current sampled translation after
composing the local scene. Freezing a translated scene into a geometry-only
snapshot rejects rather than silently losing its transformation. Inverse input
projection also respects the original local surface clip.

Verification covers unchanged geometry identity under translation, inverse
hit projection including viewport edges, zero/default behavior, boundary
durations, and GPU shader compilation/rendering where available. Native and
WASM builds must agree on the shared API.

Known ceiling: this first primitive moves the entire ordinary scene surface;
independent component transforms, scale/opacity, dynamic parent/child layout
invalidation and a product animation scheduler are still required when those
specific behaviors are implemented. It does not animate rhythm notes through
the UI dependency graph or introduce Virtual DOM.
