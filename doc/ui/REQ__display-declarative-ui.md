# Display screen declarative authoring

The Display draft screen must express its visible hierarchy, component labels,
spacing and styles together in typed Rust source. Its header, four labeled
editor rows, help text, Done/Back actions and error region must be recognizable
without reconstructing unrelated coordinate tables. Drawing and pointer hit
regions derive from the same resolved component bounds.

Migration preserves the current 960x720 logical viewport and existing layout,
action IDs, editor defaults and keyboard/navigation semantics. Pending state
removes editor/button hit admission. Invalid selected-field state rejects before
any signal write. Done edits the draft; applying settings remains separate.
Screen lifetime and navigation stay owned by the existing coordinator.

Resolve the static layout at mount. Unchanged updates reuse retained packets;
editor, focus, hover, pending and error changes rebuild only their dependent
packets. No structural declaration or layout traversal occurs on each frame.
Malformed layout dimensions, arithmetic and unsupported extents reject before
publication rather than producing divergent drawing and hit regions.

Verification includes pure layout bounds/error tests, existing Display packet
reuse/focus/pending/disposal tests and actual desktop interaction/screenshots.
The shared authoring primitives must compile for the browser target. This first
migration does not establish responsive layout or completion of other screens.
