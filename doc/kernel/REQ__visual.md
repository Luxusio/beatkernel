# Logical visual projection

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
