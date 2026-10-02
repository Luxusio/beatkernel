# Shared native audio construction

All six native solo/local compositions use `native_audio::prepare_audio` for the command queue, section-relative BGM mapping, initial admission and mixer construction. The prepared bank supplies the exact format. Platform owners pass their rendering bounds, output origin and supported start-gate choice. The existing live-command reserve, preroll/lookahead and immutable finite endpoint remain intact.

Native startup and both common gameplay loops use one `feed_rendered` helper. Only completed logical playback frames move its cursor; physical initial silence and pause displacement do not shift BGM time. Missing or paused reports perform no admission. Checked overflow, late commands, regression and queue failures retain their original failure semantics and already admitted prefix.

## Known ceiling

Native format/buffer negotiation, stream ownership and calibration still supply actual device facts. ASIO interval-aware committed startup remains unfinished. Native asset loading and remaining resource composition are subsequent work. Physical timing, real desktop playback, network and file acceptance remain deferred; compilation and authored fixtures do not establish them.

All five compile-only configurations passed on the first attempt: Linux
workspace/all targets, Windows GNU application/all targets, macOS
application/all targets, headless application/all targets and the WASM graphics
library. Dedicated `target/ac148-{host,windows,macos,headless,wasm}.exit`
artifacts contain zero. Scoped formatting and diff checks passed. Existing macOS
`block` future-compatibility and WASM cadence warnings remain.

Six source-only fixture groups use real queues, PCM and mixers for section/
preroll/origin onset and partition equality, callback capacity, initial gates
and finite endpoints, dense BGM/live reserve and FIFO order, logical-cursor
replenishment, and retained admission prefixes after failure. They were compiled
without execution; no native, socket, desktop or acoustic acceptance is inferred.
