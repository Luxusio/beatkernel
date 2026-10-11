# Browser native control layout

The existing browser setup/import/record/source controls must remain accessible
at 320px and wider CSS viewports. Labels and native input/select/button semantics,
disabled state, DOM keyboard order and visible focus outlines must be preserved.
Long chart, record, policy and device labels must not push controls outside their
panel or create horizontal document scrolling. Expand details when verifying
audio capacities, keyboard/pointer bindings and local source assignments.

Rows may wrap and controls may shrink to their available parent width. At wide
viewports, retain useful content-sized controls and horizontal arrangement where
space permits. Hiding overflow or adding JS measurement/render loops is not an
acceptable repair. Browser native option popups may use their own platform layout.

Verify document extent and individual visible control bounds at 320,390,599,600,
601,800 and1100px (one CSS pixel rounding tolerance), with long labels and all
file-input families. Inspect screenshots and exercise Tab/Shift+Tab, native select
keys and focused buttons. Geometry alone does not prove readable focus or input
behavior. Verify genuine preparation, recording/save and replay through the
existing ordinary player workflow with unchanged Worker/business/audio owners.

This control contract does not establish canvas font quality, physical input or
audio latency, full touch support or whole-player completion. Implementation and
independent acceptance belong to TASK__browser-controls-responsive-layout.
