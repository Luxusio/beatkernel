# Explicit output clock rebinding

The core estimator, native discipline and generic presentation port expose
explicit stream observation epochs and cold output rebinding.
Strictly newer epoch identity prevents tagged delayed old observations from
entering the new fit. Validated rebinding resets observation/source history and
reuses the reserved ring while preserving transport, judgment and replay state.

## Scope and evidence limits

New pairs must warm up again; this API does not seek a live judge or reset the
transport's current rate. Callers supply actual new output/playback/song origins
and retain the existing host domain. Device IO, sample-grid/PCM/voice transfer,
paused output fences, actual playback anchoring and application transition wiring
remain subsequent work. Existing untagged APIs assume the current stream and
cannot provide stale same-domain callback isolation by themselves.

Both paired writers returned terminal stop reports. Eleven fixture groups are
authored: four core, four platform-adapter and three generic-port cases. They cover
reserved ring reuse, atomic refusal/maximum epochs, stale token rejection,
new warmup/continuous history and WASAPI/ASIO metadata source/rate transitions.
Unsupported custom presentation owners
retain an explicit refusal default; the actual core/native adapters delegate
through the generic port without another observer or native side effect.
Assertions, hardware/runtime, formal review and required QA remain deferred.
The initial workspace check refused missing public-method documentation; the
source owner added those docs and stopped before scoped formatting. Final
workspace/all-targets with WebTransport, no-default-features WebTransport/
all-targets, WASM browser/library and WASM browser-audio/library checks each
exited zero in sequence. Host checks compile fixture children; WASM library
checks compile production code without those children. Existing unused-code
warnings remain. Whitespace checks completed; no assertions were executed.
Neither output clock observations nor this transition helper prove acoustic
accuracy, gapless backend replacement or instantaneous latency correction.
Full BMS player Goal stays active and unproven.
