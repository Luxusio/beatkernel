# Compose shared output ownership with native gameplay

The shared output owner connects typed replacement requests, real producer
pause holds and the existing OutputReplacement controller to the committed
paused gameplay hook. Native lifecycle and original timing remain adapter
effects; gameplay/session/input state stays in the existing pumps.

Linux solo and cohort composition now use this owner while retaining
startup, final diagnostics and output-before-input shutdown/error arbitration.
Pending replacement preserves genuine previous evidence for the old epoch and
admits candidate observations only to the staged new observer. Publication
refusals retain the complete Ready owner; explicit stop handles native resources
instead of treating unavailable output as successful completion.

The business device port exposes default-false `output_replacement_pending`.
Both pumps suppress only resume requests while an active replacement holds the
output, preserving the paused hook's polling eligibility. Normal pause requests
and adapters without replacement keep their existing behavior. The native
compatibility trait forwards the query; the owner reports active held work,
rather than treating every queued request as a running replacement.

Nine independent tests are authored: seven owner/controller cases and two actual
solo/cohort pump cases. Owner cases use actual held Mixer priming and multi-poll controller
readiness; both actual pump cases request early resume before Ready and retain
old-epoch evidence until publication. Finite owner endpoint coverage is authored
separately from the unlimited-prefix pump traces and must use real finite render
markers through owner end delegation. Full finite-pump completion remains
unverified. After both writers returned terminal `Writes STOPPED`, scoped
Rustfmt and whitespace checks completed. Four sequential compile-only checks
exited zero: workspace all-targets with WebTransport, headless runtime all-targets
with WebTransport, WASM browser lib and WASM browser-audio lib. The host checks
compiled all nine fixtures and real Linux solo/cohort compositions. Existing
unused-code warnings remain; no assertions, audio/network/browser execution,
native callback or acoustic/performance acceptance was performed.
The ASIO original-interval end override remains SDK-gated and uncompiled in the
Linux/WASM configurations. Assertions, hardware, formal review and required browser/CLI/
desktop QA remain deferred. Live UI command transport, Windows/macOS composition,
cross-backend enum composition and blocking native initialization isolation
remain follow-up work. Full player Goal remains active.
