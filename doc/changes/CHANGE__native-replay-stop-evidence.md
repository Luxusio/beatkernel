# Native recorded replay output Stop evidence

Native recorded replay now checks per-poll playback cursors and final core
diagnostics against the same retained feeder used for actual producer admission.
The explicit public completed_render_cursor_for_feeder helper permits raw
unknown_stops only within successfully admitted Stops and commands_applied.
It shares the existing diagnostic and overflow checks with strict generic and
ACK-owned cursor paths; ordinary completed_render_cursor still permits zero Stops.

The binary shares its physical/playback grid checks between the existing strict
fixture wrapper and the actual feeder-owned path. Pause, visual/hash, native
presentation, BGM/later-idle completion, cancellation and operation/stop/native/core
error ordering remain unchanged. Callback admission remains distinct from remote
ACK, execution and acoustic presentation. No platform-specific policy or replay
wire format was introduced.

Final clear/fail, profile identity and high-level mine admission remain unfinished.
Actual device/browser/performance acceptance and formal review/QA stay deferred;
the overall task and Goal remain active.

Independent deferred fixtures add four groups across the library and native
recorded binary. They cover actual fatal capture/reconstruction, rolling feeder
admission and queue/Mixer execution, future BGM and subsequent idle/presentation
barriers, partial refusal with preserved admitted prefix, equal-frame order,
strict versus owned diagnostics and physical/playback pause grids.

Both writers reported actual terminal Writes STOPPED before root formatted only
the five changed Rust paths and checked whitespace. Assertions, parsers,
applications, generated bindings, devices and formal review/QA were not executed.
All four authorized locked compile-only checks exited zero: workspace/all-targets
with webtransport, runtime no-default webtransport/all-targets, WASM browser and
WASM browser-audio. Existing unused strict-wrapper/playfield and WASM cadence
warnings remain visible. Windows/macOS compilation and physical acceptance remain
deferred; these checks do not prove actual driver or acoustic output.
