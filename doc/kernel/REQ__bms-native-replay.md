# Native recorded BMS output

The separate BMS runtime's `play_replay_bms` executable connects the checked
recorded-audio plan to the existing host WASAPI, ALSA or CoreAudio output.
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
ASIO is still unavailable until a compatible distribution/licensing path exists.
Native sound output/timing, tests, examples, reviews and QA remain unexecuted
under the user's deferral; cross-compilation proves source compatibility only.
