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
The control loop has bounded admission work. Optional `--seconds` is a checked
positive wall cutoff including preroll and may truncate the record/audio tail.
When omitted, actual recorded operations, all admitted BGM/keysound commands and
PCM voices finish, followed by an idle output block confirmed through reported
native presentation. This is an output drain policy, not acoustic confirmation.
Render progress alone cannot complete the drain; absent presentation waits for
usable observations or cancellation. Native timestamps are checked in wide
arithmetic and never replaced by render cursor, UI or receipt time. Stop/close is attempted before return on success
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
platform/sample all-target checks passed. An earlier source snapshot additionally
used target-only cfg to type-check optional Windows ASIO Rust integration and five pure publication
fixtures with Cargo SDK compilation inactive and without C++ linkage. Actual
forwarded-feature SDK/MSVC compilation, native replay output and physical timing
remain unverified. The host ignores WM_CLOSE on its hidden driver window so
message dispatch cannot destroy the system reference before native teardown.


## Graphical Watch composition

Records Watch in the single app uses this output host and the existing graphical
playfield/score bridge. It resolves omitted output metadata on the game owner;
it never enumerates or acquires keyboards. Linux defaults to ALSA `default`,
48000 Hz/stereo, buffer1024/period256; Windows resolves the active multimedia
endpoint and omitted mix rate/channels; macOS resolves the selected/default
output's nominal rate, channels (up to stereo) and current buffer. Explicit
values retain strict backend negotiation. App asset channel policy defaults to
mono-stereo; the standalone host retains its exact default.

The recording owns its profile/start; live input, judge, section, network,
ghost and capture flags from the draft are excluded. Incremental judging applies
only actual record operations in ordinal order at reported presentation time,
with no synthetic end-of-record advances or missing-tail misses. Validation of
the full log precedes audio start. F5 retains the selected chart/record/output
invocation and waits for prior-owner cleanup. Stop and final checks are attempted
on cancellation and errors, including start failures.

F9 and Pause/Resume are available for Watch when the native owner obtains an
actual output/host relation as a ClockPair or a checked interval. ALSA and
CoreAudio retain the full checked native pair; WASAPI pairs accurate
position/frequency units with the associated
host point from the same snapshot. Source-only presentation remains usable for
ordinary viewing but cannot grant pause capability. ASIO uses the complete
interval evidence and acknowledgement policy specified below.

The owner uses the same NativePause coordinator as live play and controls the
sole command producer. Pending boundaries fence command admission, recorded
operation advancement and completion while native polling, failure checks and
cancellation remain active. Actual presentation crossing acknowledges Paused;
one frozen prefix update consumes only recorded operations through that boundary.
Paused idle periods do not create repeated prefix/result updates. Resume uses the
cumulative once-rounded paused-frame gap to project physical presentation back
onto the original song grid, including recorded start and preroll exactly once.
Feeder credit uses completed playback frames; final physical render diagnostics
remain separate. Equal-time recorded operations retain ordinal order and are not
reapplied by pause/resume. Keyboard acquisition, capture and network stay absent.
F5 restarts the selected recording after cleanup; live F7/F8 bookmarks remain
unavailable. Optional seconds includes paused wall time. Native boundary mapping
quality remains Unknown, and native/GUI/timing acceptance remains unexecuted.

ASIO recorded output requires the caller's multimedia clock and timer, drift,
and latency error assessments. Actual block observations retain their assessed
host upper bounds until a fresh QPC sample reaches them; the resulting physical
presentation drives recorded visuals and natural drain. Missing observations
preserve admitted evidence without fabricating newer output. WASAPI Accurate
clock units, ALSA played estimates and CoreAudio checked presentation pairs drive
the other backends. Source checks do not prove device output or acoustic timing.

## Interval acknowledgement for recorded playback

Recorded ASIO playback uses the same NativePause and ReplayPause state machine
as point-based outputs, while retaining its original output-grid/render evidence
and complete assessed host interval. Never turn the discipline midpoint or
prepared-frame counter into pause presentation evidence. First valid use binds
one evidence kind for the session; mixed point/interval calls reject.

Recover the first physical pause/resume frame from validated mixer reports.
The request and first actual crossing observation bracket that frame. Exact
anchor equality uses its original interval; otherwise retain the conservative
[request.before, crossing.after] window without inventing a rate or precise
interpolation. Keep the first window through delayed polls, duplicate anchor
refresh and later blocks, and acknowledge only after fresh host time reaches
its latest endpoint. Metadata, chronology, report identity, arithmetic and
inconsistent grids reject atomically. Missing observations preserve evidence.

Replay freezes the exact song position derived from the boundary playback frame,
recorded start and preroll, independently of host uncertainty. Resume preserves
the cumulative once-rounded pause gap. Queued physical presentation older than
the actual resume frame cannot update visuals after the gap changes. ASIO's
bounded presentation queue accepts coherent paused/resumed grids while retaining
original maturity deadlines; it still reports physical output for final drain.

The common native replay loop consumes typed point or interval evidence without
platform branches. Audio commands, recorded operations and completion remain
fenced while pausing/paused/resuming; native polling and cancellation continue.
Control requests and observations are staged together; evidence validation and
checked song projection precede publication of state, capability and the mixer
request. Rejected evidence cannot issue a pause request or partly change phase.
Interval diagnostics retain both endpoints rather than claiming the upper
acknowledgement deadline is an exact acoustic timestamp. No live input, new
recorded operations, capture format or networking is introduced into Watch.

Offline solo/local ASIO pause uses the shared
[exact live pause policy](REQ__bms-player.md#exact-live-pause-with-interval-evidence):
earliest-bound pause input classification, latest-bound resume classification,
exact playback-frame song freeze and explicit rejection of conflicting committed
history. Those live input rules belong to the common gameplay owners; recorded
Watch continues to use the replay policy above without physical keyboard input.
Network pause remains unsupported. SDK integration is Windows+asio-sdk+MSVC;
ordinary GNU checks do not compile that branch. Pure actual-Mixer, interval,
replay and native-loop fixtures are authored and compiled only. Tests, driver,
GUI, device, physical timing, formal review and QA execution remain deferred.
