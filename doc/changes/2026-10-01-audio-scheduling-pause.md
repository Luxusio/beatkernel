# Audio scheduling pause foundation

The mixer accepts a pause request outside the command ring and freezes voices,
rational PCM phase, pending commands and queue consumption together. Paused
callbacks output silence while physical frame counters continue. Render reports
and native telemetry now distinguish physical output from playback scheduling
progress. Resume preserves the original command grid; SetRate ZERO retains its
existing separate behavior. Solo/local runtimes expose the shared audio request,
and native rolling BGM admission uses playback frames and waits during pause.

Known ceiling: this provides the audio foundation for live practice pause.
Native presentation acknowledgement, Transport/input/judging coordination,
resumed keysound mapping and deterministic replay policy remain required before
a UI pause command can be enabled. Native buffered audio can still be presented
after a render pause. Command storage remains bounded, and admission must be
fenced by the coordinator. Tests, apps, native/device/GUI execution, benchmarks
and formal review/QA remain user-deferred; authored fixtures are compiled only.

Source validation: Linux, Windows GNU and macOS workspace all-targets checks,
headless app all-targets and WASM graphics library checks succeeded. Scoped
Rust 2024 formatting and diff whitespace checks succeeded. Five mixer fixture
groups, an allocation/deallocation guard fixture and a telemetry pause/resume
round-trip fixture were authored and compiled without execution. Existing macOS
block future compatibility and WASM cadence warnings remain.
