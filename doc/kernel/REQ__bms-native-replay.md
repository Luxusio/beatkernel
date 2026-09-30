# Native recorded BMS output

The separate BMS runtime's `play_replay_bms` executable connects the checked
recorded-audio plan to host WASAPI, ALSA or CoreAudio output, or explicitly selected
optional Windows ASIO output.
It loads the matching chart, bounded WAV assets and captured replay, reconstructs
the same judge/profile and plans keysounds/BGM without native input acquisition.
Rate, channels, device and backend settings are explicit; failed negotiation
reports native requirements without selecting a replacement device or mode.

Already mapped output commands enter the existing rolling feeder using a zero
additional preroll. Output origin/domain, rate, positive lookahead and finite
command credit are explicit. Initial commands are primed before backend opening
so native prefill observes them. Later admission uses completed successful Mixer
RenderReport frame ends; timestamps are never shifted to hide lateness. Missing
coherent native telemetry is retried without inventing a new render cursor.
Queue Full/Disconnected, cursor regression, unadmitted late cues and any core
execution failure stop through cleanup. Native device/status/configuration or
callback failures likewise terminate rather than silently resume.

WASAPI uses explicit shared or exclusive mode with requested native
buffer/period frames or device defaults. Shared mode additionally exposes
`--shared-policy engine-period|legacy`, defaulting to engine-period; selecting
legacy is an explicit caller choice rather than negotiation fallback.
ALSA uses exact float32 endpoint,
period and buffer frames. CoreAudio uses exact numeric device, float32 format and
buffer frames. Backend-inapplicable CLI options reject. Queue, pending, drain,
voice and render-block capacities remain finite and independent of total notes.
The control loop has bounded admission work and a requested finite wall duration;
the duration includes preroll. No automatic end-of-song or acoustic-drain wait is
inferred from render progress. Stop/close is attempted before return on success
and errors; final admission/core/native diagnostics remain separately labeled.

Known ceiling: initial data and commands are preloaded, and dense schedules or
control stalls can exhaust finite credit/lookahead. Actual native rendering can
race a control-thread admission; late/execution errors remain observable. Original
native scheduling and past dropped audio were not captured and are not reproduced.
ASIO uses the separate [owned stream](../platform/REQ__asio-stream.md), not WASAPI
backend fallback. `--backend asio` requires a Windows host with the sample's
explicit `asio-sdk` feature. `--device` supplies a braced driver CLSID;
`--asio-view native|32|64` and distinct zero-based `--output-channels 0,1` are
required. ASCII hex case is normalized; malformed identities, absent/ambiguous
registrations, incompatible process view and wrong channel counts reject.
The actual Mixer rate must match the current driver rate. Buffer frames are
exact when supplied, otherwise driver-preferred. ASIO rejects period, mode and
shared-policy flags. ASIO-specific options reject on the other backends;
explicit backend selection must match the host. Default remains that host's
existing backend. A missing SDK feature fails before chart/replay loading.

The ASIO host owns a hidden window on the native control thread, passes its HWND
as the explicit driver system reference and retains it through stream close and
callback drain. Window/message handling occurs on the control thread. The same
checked replay, actual Mixer, rolling feeder and finite cleanup pump are reused.
ASIO fatal faults, callback render failures and overload reports terminate the
host with diagnostics. Prepared frames, successful Mixer cursor and raw native
sample/time observations remain distinct; no QPC mapping or audible timing is
fabricated. Live physical-input ASIO play requires separate presentation mapping.

Default sample features include no SDK. Its ASIO SDK-combined builds follow
the selected [GPLv3 distribution policy](../platform/REQ__asio-distribution.md)
while project-authored source remains MIT.
Native sound output/timing, tests, examples, reviews and QA remain unexecuted
under the user's deferral; cross-compilation proves source compatibility only.

Eight additional ASIO/backend/routing CLI fixtures are authored and compiled
without execution. Locked Rust 1.98.1 host workspace and default Windows GNU/macOS
platform/sample all-target checks passed. Target-only source cfg additionally
type-checked optional Windows ASIO Rust integration and five pure publication
fixtures with Cargo SDK compilation inactive and without C++ linkage. Actual
forwarded-feature SDK/MSVC compilation, native replay output and physical timing
remain unverified. The host ignores WM_CLOSE on its hidden driver window so
message dispatch cannot destroy the system reference before native teardown.
