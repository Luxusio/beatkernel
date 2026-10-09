# Logical visual projection

## Current verification boundary

The user lifted verification deferral on 2026-10-06. Earlier dated deferral and
unexecuted-example statements below describe historical evidence only.
[Long-duration visual time](REQ__long-duration-visual-time.md) records current
20-hour/week virtual traversal, independent local geometry oracles and any
confirmed numerical correction. Kernel CPU projection remains separate from
app renderer inputs and actual GPU/native presentation acceptance.

## Original projection contract

Phase 9 supplies renderer-independent Lane, Point and Path states keyed by
opaque chart visual bindings. The caller provides geometry and visible song
time windows. No GPU, skin or UI framework belongs in the kernel.

Build an interval index once; frame queries include long objects overlapping
the window and prune nonoverlapping subtrees. Reuse caller-owned object and
transient-event vectors. Geometry remains in immutable projection bindings,
so path output references a binding instead of cloning a polyline every frame.

Signed and zero compiled scroll velocities affect lane distance only. Integrate
the separate visual timeline without changing judge targets. Float arithmetic
is confined to visual coordinates and interpolation. Reject nonfinite geometry,
missing/duplicate bindings, invalid durations and reversed windows explicitly.
Each path has at least two finite points and a positive compiled duration.

The external visual example writes SVG from logical frame states. Implementation
and buildability do not establish renderer QA; verification is deferred by the
user's 2026-09-30 instruction.

## Runtime-driven final composition

The `runtime_visual` example composes virtual physical input, bindings, the actual
JudgeEngine/Runtime and Mixer command queue with lane and path projection. Each
frame uses the returned RuntimeReport song time and actual emitted judge events,
with caller-owned reusable RenderFrame storage. The external SVG renderer reads
only projected logical state, immutable path geometry and transient events;
renderer ownership and GPU operations remain outside the kernel.

The trace and frame count are finite and explicitly synthetic. The output is a
contact sheet written with create_new; an existing file is never overwritten.
This demonstrates composition source, not native acquisition, visual QA or
physical timing. Example execution remains deferred.
Rust 1.98.1 locked workspace all-target compilation passed after the example
was added. Compilation does not establish emitted SVG correctness, actual
fixture outcomes or renderer acceptance; no example or test was executed.

## Radial and caller-defined geometry

`Projection::Polar` supports the radial-approach user: finite center and angle in
radians, nonnegative target/approach radii, and positive approach duration. Its
logical radius interpolates from approach radius (progress zero) to target radius
(progress one), retaining center/angle and clamped progress. This projection uses
song time independently of scroll markers, just as Point does. The external SVG
renderer converts polar coordinates into screen coordinates; core draws nothing.

`Projection::Custom` supports the second concrete user, three-dimensional target
and path geometry owned by an application renderer. `CustomProjectionHandle`
shares an actual `Arc<dyn CustomProjection>` implementation, whose setup-time
`validate` is called for every bound object before projector construction. Its
`project` receives a borrowed object plus song/visible-window times, only for
objects selected by the existing overlap index. The implementation stores its
immutable renderer geometry; per-frame state contains no arbitrary owned payload.

`CustomRenderState` carries exact object/visual identities, a caller type tag,
an opaque renderer geometry reference, sixteen fixed scalar slots with an active
count, and normalized progress. The core requires matching identities, active
count at most sixteen, every slot finite, and progress finite within 0..1. It
rejects malformed output and clears partial objects/transient events, matching
other calculated-geometry failures. Callbacks may explicitly fail with VisualError;
panics or external side effects remain the implementation's responsibility.

Existing `VisualProjector::new`, `projection`, and `project` APIs retain their
meaning. Projector/binding clones preserve genuine shared callback ownership,
not a fabricated deep clone. Custom-handle equality means shared allocation
identity, since arbitrary implementations cannot be compared structurally.
Implementations should be pure over their borrowed context and immutable geometry;
the core cannot certify custom callbacks allocation-free or deterministic. This
is a render/control-thread extension, with no audio callback suitability claim.
The fixed custom state is Copy and RenderFrame vectors remain reusable.

The visual SVG example includes radial approaches plus an external 3D target/path
callback and renderer-owned perspective transform. Indexed narrow-window,
identity/finite rejection, setup validation, shared cloning, radial boundaries,
and frame reuse fixtures are authored alongside the API. Execution/formal QA
remain deferred under the existing user instruction.
