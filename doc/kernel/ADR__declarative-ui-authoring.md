# Declarative UI authoring over retained rendering

On 2026-10-07 the user established that UI source should resemble familiar
declarative UI: a human or AI must be able to read it, recognize the intended
design, and identify design improvements without reconstructing scattered paint
and event code. This is an authoring requirement, not permission to replace the
existing retained renderer with per-frame tree construction.

On 2026-10-07 the user explicitly confirmed continuing without Virtual DOM.
Do not introduce Virtual DOM snapshots or tree reconciliation for state,
layout changes or animation. Update retained properties directly, invalidate
affected layout when needed, and animate presentation properties separately.
Dependency tracking and layout calculation have real costs; this decision is
not a claim of zero overhead or measured performance superiority.

## Current state

`ui/atoms`, `ui/molecules` and `ui/organisms` provide reusable drawing components.
Views such as `ui/records.rs` retain geometry packets and reactive subscriptions
through `ui/retained.rs`. Most screen sources still mix absolute coordinates,
signal binding, painting and interactions. The first authoring foundation in
`ui/layout.rs` supplies typed rows, columns, positioned layers and component
leaves; `ui/display.rs` declares its complete static screen hierarchy with it.
Bindings and retained packet ownership remain explicit mount code. This is one
screen migration, not completion of the requirement across all screens.

## Intended authoring model

Keep screen structure visibly hierarchical. A screen declaration should group
sections, rows/columns, labels, buttons and repeated items in the same order that
the user sees them. Express spacing, alignment, sizing and styling next to the
component or through named reusable styles. Avoid distributing one component's
geometry, hit area and visual state across unrelated coordinate tables.

Bind displayed values and actions through explicit view-model/port inputs;
business rules, file/network/device IO, and navigation ownership remain outside
the visual declaration. Keep screen lifecycle and fragment/back-stack ownership
in the existing lifecycle layer. Component declarations must not start native IO
or invent independent screen lifetimes.

Build the structural representation when mounting a view, then keep its nodes
and update the affected properties/geometry when state changes. Preserve dirty
tracking, hit admission and cached geometry. A declarative source API does not
imply virtual DOM reconciliation, per-frame layout reconstruction or a signal
per rhythm note. Notes continue using the specialized GPU instance/time path.

Prefer plain typed Rust builders/components with explicit dependencies and
statically dispatched operations. Do not introduce a macro language, runtime
reflection, dynamic dispatch, new crate or lock merely to imitate another UI
framework. Any abstraction must earn its place through clearer authoring and
measured/verified runtime behavior; declarative syntax alone proves neither
zero overhead nor complete hexagonal isolation.

## Incremental implementation and verification

Start with reusable layout/style primitives and a single existing screen. Keep
current behavior, action IDs and lifecycle semantics during migration. Display
is the first migrated screen; its requirements and actual authoring API are
documented in [the Display requirement](../ui/REQ__display-declarative-ui.md) and
[the authoring guide](../ui/GUIDE__declarative-ui.md). Fixed sizes and section
anchors are the current scope; responsive layout remains future work.

Review the screen declaration for readable hierarchy and recognizable design.
Verify shared draw/hit bounds, resize/clip behavior, malformed-state refusal and
disposal. Check that unchanged frames reuse packets and changed properties update
only the affected nodes. Use actual desktop/browser interaction and screenshots
for the migrated surface, alongside pure layout/lifecycle tests. A folder naming
scheme or a nested code sample alone is not evidence of completion.
