# Native Windows BMS sample composition

## Local Windows groups

Repeated `--local-player ID:INTERFACE_PATH` selects 2..64 distinct keyboards and
stable positive u32 player IDs. Split the first colon so interface paths retain
their content; reject duplicate IDs/paths, missing/ambiguous attachments and any
mixed `--keyboard-path` solo override. Paths resolve against the current native
keyboard records before output starts, also checking DeviceId/handle aliases.
The existing shared/exclusive WASAPI and optional ASIO owners retain their
calibration, buffer/period policies and cleanup; each player owns an independent
actual core Runtime/judge/capture/completion, with shared BGM and audio transport.

One bounded Raw Input pump feeds original receipt-clock events into InputMerger.
Queued-message backlog suppresses deadline advancement. The common frontier
uses an explicit `--advance-lag-ns` margin (default 2ms, range 0..1s); events at
or behind a committed frontier fail explicitly. Selected removal stops the group.
Completed reports survive partial failure and are published/captured before
cleanup. Audio joins and registration closes before all per-player replay save
attempts, using stable `.p<ID>.bkr` suffixes and the existing no-overwrite policy.
Group plus network mode fails before resources; saved ghosts remain per-player.
Source integration does not prove executed Windows/ASIO/device acceptance.

`windows_bms` is a separate binary in `beatkernel-bms-runtime`; the unified
application exposes the same native composition through `play` and `player`.
This native sample loads an actual supported BMS chart
and bounded WAV bank through the shared preparation library, then composes BMS rules,
the same core JudgeEngine/Runtime, real Windows Raw Input keyboard events and an
explicit native output backend. WASAPI is the default; optional ASIO follows the
backend contract below.
No synthetic hits, tones, automatic gameplay input or GPU UI are supplied. BGM alone
is automatic; accepted head/instant stages publish preloaded keysounds. Parser warnings
are printed, and unsupported parser features remain explicit errors.

Required WASAPI options are `--chart PATH`, `--device EXACT_ENDPOINT_ID`, `--mode shared|exclusive`,
and repeated `--bind CHANNEL_HEX:HID_USAGE_HEX` for every used visible
BMS lane, including scratch. Channels retain BMS visible identities (11..19/21..29),
with hexadecimal spelling and no inferred game layout. Duplicate channels or key usages,
unsupported channels and missing used lanes fail before playback. Bindings explicitly
use `DeviceSelector::Any`: any acquired physical keyboard can play the chosen keys.
The sample does not infer a specific keyboard identity. The native window must have
focus for foreground keyboard acquisition; console results report actual grades/misses.
Omitting optional `--seconds 1..3600` plays the full song. Normal completion uses
the shared [player completion contract](REQ__bms-player.md): terminal judging,
retired BGM, a later idle mixer block and its native presentation frontier.
Explicit `--seconds` remains a diagnostic cutoff and may capture only a prefix.

## Optional live ASIO backend

`--backend asio` requires Windows and the sample's optional `asio-sdk` feature.
The default SDK-free build remains MIT and must return an explicit unavailable
error when ASIO is requested; an SDK-combined build follows the
[GPLv3 distribution policy](../platform/REQ__asio-distribution.md).
ASIO requires an exact braced driver CLSID for `--device`, explicit registry view
`--asio-view native|32|64`, and a distinct ordered `--output-channels 0,1` mapping.
The driver reports the actual positive integral sample rate; the selected output
count defines the Mixer layout. Device/rate/channel substitution is prohibited.
ASIO buffer accepts `default` (driver preferred) or exact `frames:N`. WASAPI mode,
period and shared-policy options, and nanosecond buffer requests, reject for ASIO.
ASIO-only options reject for WASAPI, whose explicit mode requirement remains.

The live timing path requires the caller's explicit `--asio-system-clock multimedia`
declaration that this driver's native timestamps use the wrapped Windows multimedia
timer. It also requires explicit nonnegative `--asio-timer-error-ns`,
`--asio-drift-error-ns` and `--asio-latency-error-ns` assessments; there is no inferred
timer precision or acoustic accuracy. `--asio-anchor-age-ns` selects a finite
positive horizon, default one second. The acquisition and error envelope must fit
within the modular clock contract. Fresh QPC/multimedia/QPC anchors are acquired
before expiry on the control thread, without changing the OS timer period.

Startup uses coherent callback/render observations and post-buffer-creation driver
output latency to obtain a finite output-frame-zero/song-minus-preroll anchor.
Ongoing ASIO observations enter the same continuous presentation discipline,
preserving actual block/rate identity and Unknown rolling-model quality. Duplicate
blocks never refresh freshness. Coarse timer plateaus likewise wait for actual
host progress without changing freshness; missing readings skip only while actual progress
remains fresh. Native faults, rate/source discontinuities and stale observations
terminate instead of substituting receipt time. Keysounds use the coherent software
prepared-frame frontier, which is distinct from audible playback position.

The shared physical-input pump, binding/JudgeEngine/Runtime, BGM feeder and accepted
operation replay capture apply to either backend. ASIO callback cleanup must finish
before Raw Input unregister and native HWND destruction. Host composition source
and compile-only fixtures do not establish SDK ABI, sound output or physical sync.
The driver sysref HWND is owned separately from the focused input window. A bounded
64-message pass services that HWND during startup and observation without consuming
physical input messages. Close/quit terminates through cleanup. Inputs before the
calibrated output frame-zero host origin are counted and excluded without retiming;
future or regressing accepted input timestamps reject. Deadline advancement waits
for that origin rather than fabricating an earlier presentation point.

Defaults are `--early-ns 150000000`, `--late-ns 150000000`, `--input-offset-ns 0`,
`--voices 256`, `--channel-policy exact`, `--buffer default`, `--period default`,
shared `--shared-policy engine`, and `--preroll-ns 3000000000`. Preroll accepts
0..10000000000 ns; input offset remains separate and is applied once by JudgeProfile.
Windows must be nonnegative; offset is signed.
`--channel-policy mono-stereo` permits only explicit mono-to-stereo duplication; no
other channel conversion is guessed. `--voices` is 1..4096 concurrent Mixer voices,
not a limit on total chart objects. Native format comes from the explicit endpoint's
mix format and is printed together with requested/applied settings. Shared engine vs
legacy initialization, exclusive mode and format/device selections never silently fall
back. Shared legacy is requested by `--shared-policy legacy`, requires default period;
shared-policy flags are rejected in exclusive mode. Buffer/period accept `default`,
`frames:N` or `ns:N` with positive checked values. Existing backend exact negotiation
validates native representability and exclusive matching rules. WasapiOptions uses its
explicit default event-driven wake and normal MMCSS priority.

BGM uses the shared [rolling admission contract](REQ__bms-bgm-admission.md).
The queue, pending storage and render drain budget are fixed at 65,536 independently
of total chart BGM count. At most 64,512 admitted BGM targets remain outstanding,
leaving 1,024 nominal live-command slots; this is not hard isolation from input bursts.
Checked output timestamps remain original song time plus preroll on output origin zero;
compiled chart/Judge targets stay unchanged. Initial admission occurs before native
open/prefill. Startup calibration continues from actual completed Mixer render ends
through the sole producer; the live loop feeds before input through Runtime::enqueue_audio
with a 256-command budget. No timestamp is changed to recover a missed cue.

--bgm-lookahead-ns accepts positive i64 nanoseconds and defaults to 3,000,000,000.
Large output buffers, control stalls or dense bursts can exhaust finite lookahead or
capacity. An unadmitted target behind the rendered cursor terminates explicitly;
choose an explicit larger horizon or restart. The schedule and PCM assets remain
preloaded, with parser/asset bounds; rolling command admission is not PCM streaming.
Feeder configuration and admitted/remaining/outstanding/deferred summary print on
normal and error exit after successful feeder construction. Admission is distinct
from actual Mixer execution, native delivery and acoustic output.
 Concurrent voice/pending/rate execution rejections and late
commands remain visible in audio snapshot counters. Queue failures report exact commands
without retry or judge rollback. No 4096-total-notes ceiling or silent voice stealing
is introduced. Shared loader file/path/PCM limits apply (64 MiB per asset, 256 MiB bank).

After native Start, two coherent accurate WASAPI device-position/QPC observations
establish the startup output-frame-zero host anchor through `WasapiPresentationClock`.
Transport maps that anchor to song `-preroll`, then starts at `Rate::NORMAL`; the
short startup slope is not retained as a permanent playback rate. The input offset
is still applied once by JudgeProfile, and compiled chart/Judge targets never move.
Every admitted BGM output timestamp remains song time plus preroll, independent
of subsequent host-to-song rate corrections.

A `PresentationDiscipline` observes real coherent running snapshots on the control
thread, seeded from an accurate nonzero presentation observation with a bounded
two-second retry. Only unavailable, inaccurate or before-presentation observations
may be skipped; terminal output status and other native/chronology/domain/frequency
failures stop the session. Each loop checks current host freshness before processing
messages, validates each acquired input host point, then updates the existing
Transport continuously at sampled current host time before advancing deadlines.
Each batch processes at most 256 native messages before returning to observation
and advancement, so continuous message arrival cannot starve clock control.
Warmup requires one second of observed span; progressing observations retain a
bounded 64-pair history spaced at least 100 ms apart, with updates at most once per
second. Unchanged positions and duplicate observations never refresh progress age.
Unavailable/degraded snapshots may be skipped only while accepted progress is at
most two seconds old; stale observations stop with an explicit error.

The default controller estimates signed rate deviation from normal, spreads phase
correction over ten seconds, limits final correction to +/-1000 ppm and rejects
base drift beyond that bound or phase error beyond 250 ms. Applied reports print
measured/correction/final signed ppm, phase nanoseconds and whether limiting was
needed. Controller quality remains Unknown: observed clock convergence is not a
measured physical accuracy or DAC latency guarantee. Continuous rate updates retain
Transport history for late events; they never seek/reset judges, change Mixer rate,
remap BGM/preroll, or replay inputs. Native discontinuity requires explicit host
resynchronization outside this sample.

Default three-second preroll provides logical startup headroom. Initial calibration
and discipline seeding each have a two-second retry cap, so delays can still exhaust
that headroom; smaller or zero preroll can allow initial BGM/notes to advance before
the gameplay pump. Choose larger explicit preroll when that bounded headroom is needed.
Progress prints remaining countdown or current song nanoseconds and a focus prompt.
Calibration failure ends the session instead of inventing receipt-time observations.
The initial finite affine interval covers the explicit cutoff, or a prepared
chart/PCM-tail extent for full-song play, plus preroll and three
seconds startup/slack only to construct the origin; ongoing queries are guarded by
fresh presentation discipline, not the lifetime of that initial slope. Startup
observations and ongoing correction still do not establish physical first-presentation
accuracy or repeatable long-run hardware synchronization. Keysounds use coherent
submitted-frame scheduling on their separate output grid with Unknown physical
presentation relation; Mixer reports lateness. Optional `--seconds` is the finite monotonic
wall duration after initial calibration and discipline seeding, including remaining
preroll countdown. Queued native messages keep their actual acquisition metadata;
no synthetic input substitutes for startup messages. Initial rolling admission supplies cues within its finite horizon before native
prefill. Startup/live admission must keep pace with real rendered progress; an
exhausted horizon fails explicitly rather than recovering missed cues.

All setup/start/calibration/pump exits stop and join the stream and release Raw Input before
native window destruction. Native resources use RAII guards for early failures. Explicit
cleanup errors remain errors; if unregister repeatedly fails, the stateless window/class
is retained rather than destroying a still-registered target. No game input is invented on
focus loss or unplug; applications requiring held-state cancellation need explicit policy.

`--help` and argument validation are portable. A non-Windows native request returns an
explicit unsupported-host error. This lane authors source and uses formatting plus host/
Windows compile checks only; execution, native delivery, tests, reviews and QA are deferred. Portable preroll
CLI boundary, actual feeder mapping/identity and checked overflow fixtures are authored inside
the binary under cfg(test), compiled but not executed.

## Supplied input delivery age

A 4,096-sample HOST-domain InputDeliveryTelemetry ring is allocated before native
devices. Eligible canonical events actually submitted to Runtime retain their original
metadata; their event point is compared with a fresh same-HOST receipt observation
after acquisition/pop and before dispatch. Excluded pre-origin or other-device events
do not enter this ring. The observer does not apply the input profile offset, clamp
event times or modify judging. Domain mismatch, future event or decreasing receipt
points fail through existing cleanup; event times from different devices may regress
without violating the delivery observer (Runtime chronology remains separate).

Windows samples fresh QPC after Raw Input decoding. Raw Input event metadata already
uses QPC receipt time, so this is **QPC RECEIPT-to-runtime software delivery age**,
not a native kernel timestamp or hardware-origin delay.

Final diagnostics after native cleanup attempts, on success and error exit, print the
explicit label, total successfully observed events and retained-window p50/p95/p99/max
nanoseconds. No retained samples prints unavailable rather than zero. This measures
supplied timestamp age, separately from Runtime process_input CPU duration; it neither
infers device polling frequency nor establishes physical input-to-sound latency. See
[the core telemetry contract](REQ__telemetry.md). Observation and summary remain on the
control thread, outside audio callbacks. Source/compile checks are not native evidence.

## Optional accepted-operation replay capture

--record-replay PATH enables the shared
[accepted-operation capture contract](REQ__bms-replay-capture.md). The path must be
nonempty and is created only after native stop/close attempts, using create_new; an
existing file is never overwritten. No replay filesystem I/O occurs in the gameplay
loop. --replay-max-records N defaults to 1,000,000 and --replay-max-bytes N to
67,108,864 (64 MiB); both require positive representable usize values. Duplicate
flags reject. Codec limits are constructed only when capture is enabled, with a
4,096-byte header and canonical input limits 65,536 encoded/32,768 payload bytes.

Capture is initialized against the pristine actual JudgeEngine immediately inside
the common startup/error boundary, before it moves into Runtime. Each actual returned
RuntimeReport is submitted once, including advances, before judge errors propagate.
Accepted input provenance and unoffset song times remain unchanged; no replacement
judge, synthetic events or extra input offset is introduced. Recording failure stops
the session through existing cleanup and preserves the helper's explicitly bounded
valid prefix. Capture's identity binds the compiled builtin judge setup/profile via
a versioned noncryptographic fingerprint, not PCM/source authenticity or hardware.

After cleanup attempts, enabled capture prints operation count, encoded byte count
and complete-session versus failed-session valid-prefix status, then saves the
canonical replay. Save failures are logged even when an earlier operation/cleanup
error takes precedence, and are returned if there is no earlier error. Setup before
capture creation has no captured file; startup failures after creation may save an
empty valid prefix. Control-thread recording may allocate and encode candidate
records. Replay contains accepted judge operations rather than physical audio output
or device latency evidence. Portable CLI fixtures are authored/compiled only.

Windows supports optional --keyboard-path EXACT_INTERFACE_PATH. An absent
option retains Any-keyboard bindings; an explicit path must resolve exactly
one attached Raw Input keyboard at session preparation. Bind its session ID,
ignore other input sources before runtime admission, and fail on removal rather
than substituting another attachment. The GUI stores native interface paths,
not enumeration-time runtime IDs. Source integration does not prove executed
keyboard filtering, removal or timing behavior.

Linux supports repeated --local-input PATH for 2..64 same-host players on the
same selected chart, using shared --bind physical positions with exact per-player
devices. This replaces --evdev in local sessions; neither is implicitly shared
between players. Common PCM/output configuration and BGM remain single owners.
Each player's voice namespace, judge, capture and completion remain independent.
The primary solo app behavior stays automatic. Linux local native reports can
attach the existing graphical bridge for independent panels. Settings Players assigns exact devices; CLI/profile imports remain supported. Network
combination still requires later integration and fails explicitly for now.


Linux --local-player ID:PATH is an alternative repeated assignment form that
preserves stable positive u32 player IDs into RuntimeGroup, graphical snapshots,
scores and per-player replay suffixes. --local-input keeps sequential IDs. Both
forms require2..64 unique devices and cannot mix or combine with solo --evdev.
Fresh native DeviceIds remain separate from stable IDs. Device aliases and
availability are checked from actual opened handles before output start.

## macOS local cohort composition

Graphical local setup shall support 2..64 players assigned to distinct positive
IORegistry keyboard identities. The game owner polls one IOHID acquisition owner
and preserves normalized Mach timestamps through the bounded merger. Each member
keeps independent judgment, score, ghost and replay state over a shared transport,
CoreAudio output and BGM. Missing, ambiguous or removed assignments and HID queue
loss fail the cohort without automatic retargeting. Group plus network admission
is rejected before native resources. Output stop and HID close precede every
member replay save attempt. Native execution acceptance remains deferred.

## Fresh practice preparation

All three native solo and local compositions accept singleton --start-ns as
strict unsigned ASCII decimal within nonnegative i64. The GUI exposes the same
native setting. Source preparation preserves original future chart targets and
retains automatic overlapping BGM using original PCM suffixes; output command
mapping subtracts the selected start exactly once before existing preroll.
Fresh output calibration retains its measured/unknown native quality. Selecting
before zero, beyond remaining content or beyond PCM capacities fails explicitly.
Default zero retains the original full-song path. No preceding keysound/held-key
state is inferred. See the section-restart contract for policy and pending limits.
