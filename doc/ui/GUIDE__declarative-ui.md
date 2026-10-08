# Authoring retained screens with static layout

The parallel completion foundation adds persistent `MountedLayout` to these
initial static primitives. Mount the typed declaration once; stable preorder
`NodeId` and `LayoutUpdate` address existing size/origin/gap/clip properties.
`Node::fill` consumes remaining parent axes. Explicit updates/resize stage
checked dependent geometry and clips before publishing, preserve identity and
leave rejected changes atomic. Zero extent suspends; repeated unchanged extent
reuses packets. Bind painting and hits to the same geometry through retained
layout dependencies. Do not rebuild a virtual declaration on each frame.

Display demonstrates the new API while retaining exact default 960x720 output.
Other screen migrations must preserve native/browser capability projections,
original control IDs, current keyboard/editor/lifecycle behavior and packet
reuse. The older static-only ceiling below describes the initial milestone;
text measurement/automatic wrapping and component animation remain separate
requirements until their actual implementations and interaction evidence exist.

`app/src/ui/layout.rs` supplies typed `Node` declarations:
`leaf` identifies a component, `row` and `column` arrange children with a gap,
and `layer` positions sections with `.at(x, y)`. Each node declares its size.
The component payload is a screen-owned Rust type; layout has no knowledge of
business rules, signals, navigation or native IO.

Keep a screen's hierarchy and labels in one declaration, with named styles
nearby. See the `SCREEN` declaration in `ui/display.rs`: header, labeled editor
rows, help groups, footer actions and error region appear in visual order.
Absolute anchors currently locate sections inside the fixed logical viewport;
rows and columns express placement inside those sections.

Call `resolve` at mount, before publishing the view. It returns leaves in
declaration order with checked bounds, rejecting nonpositive sizes, negative
origins/gaps, arithmetic overflow, children outside their parent, depth above
32 or more than 1024 nodes. It owns no frame loop. Use the same resolved bounds
for component painting and pointer hits.

Mount code binds each retained packet to its explicit state dependencies.
Frame updates change signals, and composition reuses the existing geometry;
neither path rebuilds or resolves the declaration. The navigator still owns
screen instances and disposal. Rhythm notes retain their specialized GPU path.

Virtual DOM snapshots and tree reconciliation are excluded by the confirmed
design direction, including for animation and layout changes. Reactivity binds
state to existing properties; future dynamic layout must invalidate affected
retained regions rather than construct replacement virtual trees.

For scene-wide movement, construct `scene::UiTranslation` and set it with
`Scene::set_ui_translation`. The rectangle renderer applies its offset through
the existing viewport uniform, leaving cached rectangle/batch data and geometry
upload identity unchanged. `Scene::project_ui_point` applies the inverse offset
inside both the visible viewport and the original local surface clip; the
desktop pointer adapter uses it after physical-to-logical projection.

`ui::motion::TranslationMotion` samples a linear integer-pixel offset from a
caller-supplied `Duration`. It owns no timer, clock or scheduling loop. Zero
duration completes immediately; elapsed time past the endpoint clamps to the
destination. Intermediate displacement truncates toward zero, rounding toward
the starting offset; this is integer-pixel movement, not subpixel interpolation.
Sampling supports `Duration::MAX` without arithmetic overflow. The presentation
owner composes local geometry, applies the
sampled offset and requests a redraw. Clearing a scene resets translation;
geometry snapshots reject nonzero translation instead of losing it silently.
Offsets beyond ±2^24 logical pixels reject to preserve exact integer uniform
representation. This primitive moves all ordinary scene geometry together;
individual component transforms and dynamic sibling layout remain future work.

Display keeps its existing packet groups and public button definitions because
desktop hover projection consumes those definitions. Its migration tests must
cover agreement between resolved actions and those shared button bounds, along
with editor focus, pending hit suppression, unchanged packet reuse and disposal.

Known ceiling: this foundation supports fixed explicit sizes and section
anchors, not responsive sizing, text measurement or automatic wrapping — extend
it when a concrete screen requires those behaviors and add corresponding pure
layout and rendered interaction checks. Other screens still use their existing
authoring paths. This API does not establish universal zero overhead or complete
screen migration.
