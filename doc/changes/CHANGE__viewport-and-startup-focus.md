# Preserve captured coordinates and fence startup focus

Viewport projection preserves an identity axis directly, avoiding a needless
divide/multiply round trip that changed fractional captured off-surface points.
Other axes multiply before division with a divide-first fallback when only the
intermediate product overflows. Final nonfinite results remain errors; menu
clipping, half-open edges and raw contact ownership do not change.

Search focus acquisition now requires ready UI, Selection and no pending
catalog. Lifecycle clearing remains permitted while startup/render/profile/UI
state is unready, so cleanup does not retain focus behind an admission fence.

Existing viewport and pending-profile startup regressions failed before source
changes. All three viewport tests passed afterward, including the new extreme
finite identity/downscale case. All four startup tests passed. Full main suite:
202 passed / 17 prior failures; full library: 1469 passed / 95 prior failures.
No unknown failure names occurred. Workspace all-targets WebTransport, WASM
browser and whitespace checks exited zero with only existing warnings.
No actual browser/native pointer or GPU acceptance
is inferred; independent reviews and final QA remain outstanding.
