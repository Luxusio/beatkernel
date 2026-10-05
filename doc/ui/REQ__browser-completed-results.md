# Worker-owned completed results

Browser live solo and local players use actual StepGameplay/StepLocalGameplay
completion evidence to freeze a separate completed presentation before their
gameplay object is freed. Scores or a JavaScript completion choice cannot create
that evidence. A cancelled session without proof and recorded replay prefixes
must not produce a whole-song completed result. The first result remains usable
through later cleanup failures, and a complete local table requires all original
registered players rather than a partial or aggregate gauge.

The frozen presentation uses the common retained native Results component,
including exact gauges, full/practice scope, score and comparison prefixes.
It does not retain live note, audio, input or sample owners. Results navigation
and rendering start only after the existing stop/cleanup acknowledgement; room
drain retains its original lifecycle and footer alongside local results.

The Worker owns result rendering, disposal and page/mode admission. It checks
original play identity and increasing RPC identity and refuses malformed pages
atomically. New selection/play, reset and fatal disposal invalidate old results.
The Window forwards bounded UI gestures and finite result metadata; it does not
render results or run a result rendering loop. Mode/page switches reuse cached
packets. Large counters and timestamps remain exact Rust integers or BigInt
across JS boundaries. Historical completion is separate from technical errors
and does not establish a verified remote opponent clear.

Independent deferred fixtures cover actual portable completion extraction,
incomplete/cohort rejection, first evidence preservation, JavaScript admission,
cleanup barriers, stale identities, page/mode transitions and disposal. The
standing verification deferral applies: assertions, JS parsers, generated WASM,
browser/renderer/device execution and performance checks remain deferred.
Compile-only checks cannot establish runtime or rendering acceptance.
