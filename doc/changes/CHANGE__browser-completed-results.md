# Browser completed presentation

Browser completion presentation derives from the actual stepped gameplay owner's
immutable solo or whole-roster results. It freezes common score/comparison page
geometry before releasing gameplay and sample owners. The Worker retains that
presentation separately and renders it through the existing Worker-owned wgpu
canvas after the Window's existing gameplay/audio cleanup path settles and
acknowledges presentation. Window code forwards finite control gestures and
metadata without result rendering. Full-song and practice results, cancellation
without proof, replay prefixes and technical cleanup errors remain distinct.

See [the behavior requirement](../ui/REQ__browser-completed-results.md).
Thirteen independent fixture groups were authored: four actual portable
StepGameplay/StepLocalGameplay and Mixer groups, three JS admission-model groups,
and six actual Worker VM groups. Two existing Worker harnesses received only the
new mock method and actual helper-module linker needed by this source change.
Root scoped rustfmt and whitespace checks succeeded. Four compile-only checks
succeeded: workspace/all targets with webtransport, headless/all targets with
webtransport, WASM browser and WASM browser-audio. Rust fixtures were compiled;
the nine JS groups were authored without parsing or execution. Existing dead-code
warnings remain. Browser execution, JS parsing, assertions, generated
bindings/WASM execution, GPU and performance acceptance are deferred. The full
Goal and required ordered reviews/QA remain open; no runtime PASS is claimed.

## Known ceiling

Actual room drain/footer behavior, exhaustive fatal invalidation, nonzero timing
score capture, generated WASM, browser/renderer, allocation and performance
acceptance remain unverified. JS fixtures use the actual Worker and model sources
with generated-binding and browser boundary mocks; they are not real browser
evidence. Historical results remain available through cleanup errors while their
Worker owner remains usable; fatal Worker disposal cannot preserve an in-memory
result for later recovery. Archive persistence is still follow-up work.
