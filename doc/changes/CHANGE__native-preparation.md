# Shared native preparation

Live play and replay watching now use `native_defaults::prepare` to resolve omitted defaults. The owner validates the original invocation before querying native metadata, requests only missing fields, and preserves explicit device/rate/buffer options. Native discovery supplies actual default output IDs, output formats and keyboard candidates. Solo live play can select a keyboard automatically; assigned local cohorts retain their choices, and watching a replay never queries keyboards.

The shared owner retains backend semantics: ALSA uses its default PCM and existing format defaults; WASAPI live setup owns format selection while replay may query mix format; CoreAudio supplies omitted device/format/buffer defaults; ASIO requires an explicit driver and preserves routing and caller clock assessments during replay projection. Invalid native metadata and unavailable defaults return errors rather than becoming synthetic device identities.

## Known ceiling

Stream/member resource construction and some native setup/cleanup composition remain platform-owned. ASIO committed startup still needs bounded-interval timing support; reducing its uncertainty to a fabricated point is not equivalent. Native device availability, physical timing, desktop playback and real multiplayer execution remain unverified. Tests and formal review/QA execution are deferred by the user.

All five compile-only configurations passed: Linux workspace/all targets, Windows GNU application/all targets, macOS application/all targets, headless application/all targets and the WASM graphics library. A subsequent fixture-only correction uses accepted Windows option syntax; the application library/tests compile check also passed afterward. Dedicated `target/ac144-{host,windows,macos,headless,wasm}.exit` and `target/ac144-fixtures-2.exit` artifacts each contain zero. Scoped formatting and diff checks passed. Existing macOS `block` future-compatibility and WASM cadence dead-code warnings remain.

Five new recording-adapter fixture groups cover the host/mode query matrix, preserved explicit options and player assignments, validation before discovery, invalid metadata admission and ASIO projection/live-query behavior. These fixtures were compiled without execution; no native, socket, desktop or formal acceptance is inferred.
