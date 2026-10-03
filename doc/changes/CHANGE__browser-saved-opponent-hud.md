# Worker-owned saved opponent HUD

Saved-record comparison prefixes are retained in a bounded portable HUD snapshot
and presented by the graphics Worker using the common competition scoreboard.
The input/audio path and local score remain independent. Normal periodic
comparison updates no longer cause Window DOM counter writes. Comparison
failure hides invalid HUD state and reports once, while local play continues.

Stopping captures one final actual comparison prefix before owner disposal and
includes it in the correlated stop/error receipt. Window presents final results
after joined cleanup for the current session and page only. Unrecorded tails
and independently failed comparisons are not promoted to complete results.

Production and seven independent deferred groups are authored: three portable
Rust groups, two Worker groups and two Window groups. Existing fixtures remain.
An initial headless compile exposed graphics-only fixture imports; the repair
gates only geometry checks while retaining all three portable groups. Workspace,
headless, browser WASM and browser-audio WASM cargo checks completed with exit
code zero after applicable repair checks. Existing WASM cadence warnings remain.
Scoped formatting and whitespace checks completed. Tests,
JavaScript parsing, generated bindings, browser/device/runtime execution,
formal reviews and QA remain deferred. The full player Goal, network HUD/host
work and measured main-thread performance acceptance remain outstanding.
