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

Independent retained components extend this surface primitive with fractional
translation, scale, opacity and easing. A component belongs to the actual screen
instance and retained node; its renderer slot also carries an allocation epoch.
Changing transforms updates component uniforms without rebuilding rectangle
packets or indexed-note geometry. Paint and inverse pointer projection must use
the same transform and source/parent clips; zero opacity cannot receive hits.
Scale uses the retained node-bounds top-left as its pivot. Indexed note
layers are painter-order insertion boundaries: ordinary component ranges may
end or begin at that boundary, but cannot span it. Empty geometry associations
remain valid for clipped or hidden retained nodes so identity survives updates.

The bounded scheduler consumes caller-supplied monotonic durations. Suspension
freezes progress, resumption excludes suspended time, cancellation removes the
owned track, and disposal prevents later scheduling or ticks. Invalid transforms,
stale component identities, cross-screen ownership and capacity exhaustion refuse
atomically. The 64-component bound applies to actively bound motion components,
not all mounted retained nodes. Rhythm notes remain outside this UI scheduler.

Known ceiling: component source, deterministic fixtures and Selection/Display
browser and native Desktop menu-owner integration are implemented. Native
window/GPU motion has software Vulkan evidence; hardware performance and
independent GUI QA remain pending. The other
shared menu views retain their existing rendering behavior; their control-motion
requests are explicitly unsupported. Source APIs and mocked worker tests do not
establish all-screen product animations or hardware input/paint parity.

Native Desktop motion must be requested explicitly through a typed host command
for the actual screen instance and displayed control. It reuses mounted-node
composition and the bounded shared scheduler. No CLI animation flags or automatic
hover/entrance policy are required. The native adapter may access public retained
control/node associations and immutable presented poses across the app library
boundary; these are presentation contracts, not gameplay clock authority.
An immutable scene geometry identity/revision is available to native adapters
for checking cache reuse. Animation-only ticks must preserve both the identity
and revision; reconstructing geometry is reserved for actual layout changes.

## Practice motion contract — acceptance verification in progress

Extend the same explicit control-motion capability to the current Practice
screen instance. Start/end editors 70/75 and actions 71/72/73/76 resolve their
actual retained nodes; hidden or stale controls refuse. Browser and native
owners reuse the existing bounded scheduler and component composition. Motion
ticks change uniforms without rebuilding geometry or changing business revision.
Only successful presentation publishes the new pointer pose. Zero extent,
surface retry and unpublished ticks retain the prior accepted pose. Back removes
Practice and its tracks while resuming retained ancestors; reopening Practice
creates a new instance.

Native IME candidate rectangles must follow the accepted painted editor pose,
including fractional movement, scale, source/parent clips, scene translation,
opacity and viewport letterboxing. The shared immutable pose model supplies
visible source-rectangle projection. Component keys absent from that snapshot
refuse; genuinely unanimated staged controls carry no component key. A failed
keyed projection must not fall back to an unanimated rectangle. Immutable prior
snapshots remain valid after the live scene changes or disposes bindings.
Motion-owned physical projection uses the accepted frame extent and outward
rounding, preserving focus, Tab, preedit/commit and existing action meanings.

Verify actual Practice owners, editor repaint/rebinding, unpresented versus
accepted hits and candidate anchors, hidden/zero-opacity refusal, stale requests,
screen removal and unchanged geometry identity/revision. Real current WASM
Worker and native window observations remain separate from pure fixtures.
This section specifies intended behavior; source integration is implemented,
while actual painted-frame acceptance and fresh independent review/QA are
pending. Other screen motion and physical performance obligations
remain in WBS10.16/10.17.

Foundation development verification (2026-10-10): the graphics-enabled app
library's Practice filter passes 118 tests, including all four new retained
component facade fixtures. The new `UiPresentedPose::visible_rect` filter
executes five tests and passes all five, covering literal fractional/pivot
projection, source/parent/viewport clipping, opacity, invalid rectangles and
immutable snapshots after live changes/disposal. The original two failing
Practice fixture inputs moved actions outside their fixed parent row; corrected
horizontal probes retain strict clipping and geometry identity assertions.
These results verify the shared foundations only. Browser/native owner
fixtures subsequently pass five browser cases and seven native cases. Existing
browser-menu, shared component-scene and native-related filters pass 29, 10 and
49 cases respectively; overlapping filters are not unique-test totals. Native
window tests remain ignored unless explicitly enabled. Actual painted Practice
frames and independent review/QA remain pending.
Logs are machine-local under `target/wf/practice-component-motion-01a12429/`.

Native development evidence: `native-component-motion-check-development.log`
passes the native app binary type check. The focused binary fixture run passes
8 tests / fails 0 (`native-component-motion-focused-development.log`), including
five new actual Desktop owner cases for typed command replies, accepted pose
input, immutable geometry identity/revision, refusal atomicity, Back/zero-extent
timing and fixed Display clipping/disposal. These renderer-less fixtures do not
establish actual native GPU presentation; fresh independent review and GUI QA
remain required.

The combined native binary suite passes 281 tests / fails 0 / ignores 3
(`native-component-motion-bin-development.log`), preserving existing native
navigation, editor, catalog and lifecycle fixtures alongside the new motion
cases. Ignored native/window cases remain outside this result.

Actual native window verification separately passes 1 ignored test when explicitly
enabled (`native-component-motion-x11-development.log`). The real X11 window,
Desktop draw path and llvmpipe Vulkan/Fifo renderer submit 22 animation frames
(presentation counters 1..23), move Selection control 5 by -200 pixels, reject
the previous hit position and accept the moved position while preserving geometry
identity/revision. The owner disposes its components and closes the window.
This proves software native rendering functionality, not hardware performance,
real physical input acquisition or full independent desktop QA. The screenshot
captured after window closure was blank and is not visual acceptance evidence.
Component bindings must carry the actual inherited parent clip separately from
the node's bounds and node-local source clip. Ancestor clip changes invalidate
this dependency even when leaf geometry stays identical. A node-local clip
moves and scales with its component; scaling still pivots at the node-bounds
origin. Cold component composition must retain source geometry and hit regions
that a later movement can reveal, rather than permanently crop them against
the old ancestor or viewport. Unanimated composition retains its current
clipping behavior. The clip correction and independent development fixtures are implemented;
Selection/Display browser integration has software GPU evidence. Native Desktop
integration has development fixtures and software Vulkan window execution;
hardware performance and independent GUI QA remain required.

Browser menu motion is explicitly requested for an actual displayed control,
using its owning screen instance and retained node. The renderer supplies
monotonic presentation time, independent of audio/input clocks. Animation ticks
must continue without requiring a new business menu revision. Suspended parent
screens retain pose and elapsed progress; Back resumes them, while removed
screens dispose their tracks. Zero backing extent suspends progress. Rebinding
after navigation must not exhaust slots belonging to inactive screens.

Hit testing uses the pose from the last successfully presented frame. Surface
retry or zero extent cannot publish a new input pose that was not painted.
Requests with stale ownership, invalid transforms or regressing time refuse
without changing admitted motion. No automatic animation duration or effect
is selected by this API; native/browser nonzero rendering evidence remains
required after actual host integration.

Development evidence (2026-10-08): app library 2077 PASS / fail 0 / ignored 4
(`browser-component-motion-app-final-development.log`); browser Node tests
692 PASS / fail 0 (`browser-all-component-motion-development.log`). Production
browser WASM build and pinned wasm-bindgen 0.2.129 regeneration pass; the generated
package exposes explicit request, timed draw, active-motion, suspend/resume and
dispose methods, and its real menu snapshot decodes successfully. Menu lifecycle,
submitted-pose inverse hits, fixed parent clipping, slot rebinding and separate
animation/surface-retry scheduling are covered at pure/worker boundaries.
Real GPU presentation and independent Harness review/QA remain required.

Actual browser development evidence (2026-10-08): an isolated Chromium with
SwiftShader (`vendor=google`, `architecture=swiftshader`, fallback adapter) runs
the regenerated WASM and real renderer Worker through OffscreenCanvas. A caller
requests Selection SETTINGS control 5 to translate from x750 to x550 over
800 ms. The real worker submits 51 frames without render errors. Protocol point
probes accept control 5 at its original position before motion, refuse that old
position after motion, and accept the moved position. Before/after screenshots
and `browser-software-motion-evidence.json` are retained under the task's
`target/wf/parallel-player-requirements/` directory. The renderer's dispose
acknowledgement, owned browser exit and local server shutdown are confirmed.
This is actual software WebGPU functional evidence, not native Desktop proof,
hardware performance, full product flow or independent qa-browser acceptance.
