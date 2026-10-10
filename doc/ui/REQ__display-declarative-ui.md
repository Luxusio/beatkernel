# Display screen declarative authoring

## Native screen ownership

One existing PanelScope owns both the Display draft and its lazily mounted
retained view under the same screen instance. Parent retention preserves both;
Back or removal retires both, and reopening creates a fresh owner. Keep the
view's drop before the draft, cancellation permits and accepted geometry/hit
identity. Editing, IME and clipboard access the same owned draft; stale owners
cannot update it. Cancel preserves committed display settings, while existing
Done/apply behavior remains authoritative.

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

## Next retained dynamic-layout requirement

The parent-Goal parallel implementation extends this initial static milestone.
Keep mounted node identity and bounded hierarchy/dependencies after resolution;
explicit extent, child-size, parent allocation and sibling changes reflow only
affected geometry. Paint and hit admission use identical published bounds/clips.
Invalid geometry leaves the previous published state unchanged; zero extent
suspends output. Unchanged frames reuse packets without rebuilding declarations.
The original 960x720 layout remains the compatible default, rather than the only
accepted extent. Preserve current depth/node bounds unless explicitly revised.

Display must exercise actual reflow and shared input geometry. Independent
Selection/Settings/Records/Practice/Players/Devices/Results authoring then adopts
the frozen API; Display-only completion does not complete all-screen migration.
No Virtual DOM, scene transfer or orphan animation lifecycle is introduced.
