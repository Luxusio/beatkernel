# Authoring retained screens with static layout

`samples/bms-runtime/src/ui/layout.rs` supplies typed `Node` declarations:
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
