# Portable processing clock and AudioWorklet Mixer ownership

The common Runtime can select a caller-supplied monotonic profiling clock or
disable processing-duration measurement. Native profiling still defaults to
`std::time::Instant`. Missing or regressing external readings do not become
zero-duration samples. Rejection counters and existing telemetry are retained.
RuntimeGroup and SoloRuntime forward the selection to their actual members;
binding, judging, transport, replay and queue operations use the same code.

The existing application crate adds a portable WorkletAudio owner and the
optional graphics-free `browser-audio` binding feature. Setup owns original-rate
PCM, the real core Mixer and its sole bounded queue. Output storage is allocated
once. One absolute AudioContext start establishes relative Mixer frame zero,
including a silent prefix when the first active block crosses the start.
Block sizes are explicit rather than assumed to be 128 frames. Invalid blocks,
context gaps/regressions, overflow and render errors fence the owner and clear
output. Startup validation rejects stale or duplicate arm requests atomically.

Numeric WASM rendering takes absolute frame low/high words and returns status;
it returns no vector, string or report object on each callback. The JavaScript
processor copies retained interleaved output into browser-owned planar channels.
Reports are requested through control messages and contain actual Mixer evidence.
Ordered command acknowledgements distinguish queue admission from rendering,
preserve admitted prefixes on failure, and never retry committed gameplay.
PCM admission, initialization, polling and destruction stay outside `process`.
Unexpected WASM memory growth after activation terminates the processor.

The processor loads a bounded nonstreaming UTF-8 bootstrap before the pinned
generated bindings. Native encoding globals remain in use when available.
The fallback supports the generated bindings' scalar replacement, malformed
input, BOM/fatal behavior, typed-buffer offsets and encodeInto counts; unsupported
encodings or streaming fail explicitly. It is not a generic encoding framework.

The browser still exposes a chart preview. The sample/control host bridge,
nonblocking shared gameplay owner, DOM input watermarks, output-presentation
mapping, capture/replay and end-user start/stop controls remain required work.
This component provides no native WASAPI/ASIO device control, hard realtime
guarantee or measured acoustic accuracy. The full player Goal remains active.

The durable contracts are [Runtime](../kernel/REQ__runtime.md) and
[browser ownership](../kernel/REQ__bms-browser.md). Build commands for separate
graphics/audio generated artifacts are in the
[browser README](../../samples/bms-runtime/web/README.md).

## Verification status

Seven locked Rust 1.98.1 configurations completed with exit 0: host workspace
all targets, Windows GNU, macOS, headless, WASM graphics, WASM browser and the
new graphics-free WASM browser-audio feature. Eleven Rust fixture groups are
authored and compiled: four actual Runtime groups and seven actual Mixer groups.
Fifteen JavaScript groups are authored for the real processor with mocked WASM
ownership and the UTF-8 bootstrap with native encoding oracles. JavaScript has
not been executed or syntax-checked. Existing WASM native-cadence warnings and
macOS block future-compatibility warning remain; SDK/MSVC ASIO is outside these
checks. Logs are `target/ac177-{host,windows,macos,headless,wasm,browser,worklet}`.

Assertions, generated bindings, WASM linking, JavaScript/Worklet
execution, browser/native audio/device testing and formal review/QA remain
deferred by user instruction. Compilation is not an acceptance PASS. The next
user steering selects QUIC multiplayer transport; its migration is recorded
separately and does not change the incomplete browser gameplay boundary.
