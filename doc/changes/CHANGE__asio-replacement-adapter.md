# Bind ASIO replacement using original interval evidence

The SDK-gated ASIO adapter calls real driver open and recoverable preparation
through a sealed native trusted-opener capability, keeping the app library's
unsafe-code prohibition intact. Original errors, recovered mixer and pending
owner diagnostics stay separate. Creation epochs use actual pre-priming frame
bases; basis-aware admission distinguishes staged stream zero from original
absolute render origin without relabeling evidence or adding offsets twice.

## Evidence

Implementation and eight independent tests are authored: six portable real
render/interval admission cases and two SDK-only error/mixer mappings.
Cases include staged advanced-origin regression, original bounds, nonzero/max
epochs, source/basis identity, duplicates/coarse-host suppression, atomic refusal
and retained queued PCM. Native-only tests do not fabricate controls or clocks.
Only genuine admitted intervals replace the adapter cache. The existing Windows
gameplay interval helper delegates to shared projection with original render
identity and complete host bounds. Tests avoid fabricated native controls,
streams or clocks. Assertions/runtime/formal review and QA remain deferred;
SDK source and target-only fixtures are uncompiled by the four Linux/WASM checks.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The six portable tests and existing Windows-binary interval delegation compiled
in the host workspace check. SDK adapter/capability/stream getter and two gated
tests remain uncompiled; no assertions or fixture effects ran. Existing unused
code warnings remain. A missing gated error helper was factored from the actual
open path before textual fixture alignment, without pretending target validation.

## Known ceiling

Driver timestamp assumptions need caller-supplied multimedia anchoring and honest
latency bounds. Native capability lifetime, SDK/driver lifecycle, pending-owner
mapping and device/acoustic acceptance remain unverified. Replacement play-loop/
UI composition and blocking-call isolation remain pending. Full BMS player Goal
stays active and incomplete.
