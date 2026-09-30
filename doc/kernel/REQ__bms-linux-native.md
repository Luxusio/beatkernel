# Native Linux BMS sample composition

`linux_bms` separately composes the shared bounded BMS/WAV loader, real compiled
BMS rules/chart, core Runtime/JudgeEngine, one explicitly opened evdev node and
an exact ALSA float32 output stream. No automatic gameplay input, synthetic tones,
keyboard-device guessing or lost-input reconciliation is supplied. BGM is automatic;
accepted instant/head stages trigger actual referenced WAV keysounds. Parser warnings
are printed. The existing offline and Windows binaries remain independent.

Required CLI options are `--chart PATH --evdev NODE --alsa ENDPOINT --rate HZ
--channels N --period-frames N --buffer-frames N --seconds 1..3600`, and repeated
`--bind CHANNEL_HEX:HID_USAGE_HEX` covering every used BMS lane including scratch.
Channel codes are explicitly 11..19/21..29; nonzero physical keyboard HID usages
are hexadecimal. Duplicate lane/key bindings, missing used lanes, unknown options
and invalid numeric requests fail explicitly. This single acquired node has session
DeviceId(1), and every binding uses `DeviceSelector::Exact(DeviceId(1))`; other
keyboards are not implicitly selected. evdev acquisition does not require a GUI
focus window or exclusive device grab. Device permissions and native capabilities
remain explicit errors. Applications must choose an appropriate node and manage
its relationship to desktop input themselves.

Defaults mirror Windows: early/late windows 150000000 ns, signed input offset0,
preroll3000000000 ns (allowed0..10000000000), voices256 (allowed1..4096 concurrent),
and `--channel-policy exact`. `mono-stereo` explicitly permits only mono-to-stereo
WAV duplication. Offset applies once in JudgeProfile; compiled targets stay unchanged.
Output encoding is float32, with exact requested rate/channels/period/buffer and
`allow_size_rounding=false`; unsupported formats/configuration never fall back.
Period must be positive, less than buffer, and at most the Mixer render ceiling
1,048,576 frames. Requested/applied settings and every canonical binding are printed.

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
There is no total-note-count limit of4096, implicit voice stealing or streaming claim.
Queue failures preserve exact command/reason and do not undo grading or retry. Active
Mixer voices remain separately bounded. After stop/join, the sample prints the
last successful typed RenderReport and its actual cumulative Mixer counters:
commands consumed/applied, lateness, pending/voice capacity, unknown samples/stops
and invalid gains/rates/times, plus active voices and pending commands. None is
explicitly unavailable, never fabricated as zero. See the platform
[ALSA render telemetry contract](../platform/REQ__alsa-render-telemetry.md).
The retained successful report survives terminal output cleanup and is printed
before returning gameplay/start errors. It does not describe a failed render or
prove native write completion, device presentation, or physical sound. Successful
queue admission, Mixer execution and native submitted-frame/error counters remain
distinct observations; render rejections do not retry inputs or undo grading. Shared loader asset/path/PCM limits
apply:64 MiB per asset,256 MiB bank. Samples and mixing are preallocated off-thread.

After Start, startup polls actual separately coherent native ALSA timing within a
two-second bound. Ready is permitted only during startup; terminal failures stop.
`alsa_presentation_pair` supplies an optional checked output-frame-grid/native
CLOCK_MONOTONIC pair from estimated played frames, native timestamp and applied
sample rate. Missing timing is not replaced by aggregate counters or current receipt
time. The first valid pair estimates output-frame-zero host time as native host time
minus output elapsed time, using checked wide arithmetic. Transport starts NORMAL at
that estimated host origin and song `-preroll`; this origin and all timing quality are
explicitly Unknown, not a calibrated acoustic or physical first-presentation claim.

The bounded PresentationDiscipline receives those real supplied pairs, observes
freshness every loop, retains decimated progressing pairs and continuously updates
only the host-to-song Transport before advancing deadlines. Duplicate/unchanged output
positions never refresh progress; unavailable timing is permitted only while accepted
progress remains fresh. Current host and each acquired input host point are validated.
Default observation span1s, retention100ms, update interval1s, correction horizon10s,
maximum progress age2s, maximum phase250ms and final rate range+/-1000ppm apply. Reports
print measured/correction/applied signed ppm, phase, limiting and Unknown quality.
Native resets/regression/domain/rate/phase/freshness errors require terminal cleanup
and explicit restart, not judge reset/seek, BGM/preroll remapping or Mixer rate change.

Each loop handles at most256 evdev items. Actual canonical events pass through the
same Runtime; `SYN_DROPPED` or a Resync barrier ends the session without fabricated
release events or automatic acknowledgement. Unsupported/synchronization records
are consumed explicitly; WouldBlock ends that batch. Key sound scheduling uses a
conservative ceiling of the independently observed rendered-frame cursor so it does
not intentionally target a period already rendered. This scalar scheduling observation
is separate from native presentation; aggregate ALSA snapshot fields are independent
atomic readings, not one coherent presentation snapshot. A running worker can still
advance before admission, so Mixer lateness and physical timing remain unverified.

Seconds is the finite wall duration after startup seeding, including remaining preroll.
Countdown/current logical song and grades/misses are printed. Zero preroll can allow
startup to consume initial BGM/notes; default3s provides logical startup headroom rather
than physical sync guarantees. Native status/xruns/suspends/errors and runtime software
processing percentiles/counters are reported. All loop/start failures explicitly stop
and join output before dropping evdev; stream RAII also protects setup failures. No
silent ALSA xrun recovery or synthesized input is introduced.

Portable `--help` and argument validation open no devices. Non-Linux execution returns
an explicit unsupported-host error. CLI/preroll/origin/boundary fixtures are authored
under cfg(test), compiled but not run. Only formatting and host/Windows compile checks
are performed in this lane; native execution, tests, reviews and formal QA are deferred.

## evdev backlog and deadline watermark

`--advance-lag-ns` defaults to2000000 ns and accepts0..1000000000. Events retain
original kernel host timestamps and are judged immediately; none are retimestamped
to receipt time or clamped to a deadline. A batch reaching256 items without
WouldBlock defers deadline advance while still observing/updating presentation.
When the node is drained, deadline host watermark is the maximum of estimated
initial output origin, last accepted operation/input host time, and current host
minus lag (computed in checked wide integers). Lag delays timeout processing; it
does not alter the grading input offset. Explicit zero lag permits immediate
current-time deadlines and can overtake subsequently arriving kernel data.

Kernel events acquired before the estimated audio origin are explicitly counted
and ignored with diagnostics, rather than inserted into the new audio epoch.
Each event is also checked against a fresh CLOCK_MONOTONIC reading after its
read; a future kernel timestamp fails rather than allowing grading ahead of the
current correction anchor. After that epoch begins, events older than an already accepted operation remain
explicit chronology failures requiring cleanup/restart. Finite lag assumes bounded
kernel delivery delay and is not a guarantee against arbitrary backlog. Loss/resync
is always terminal regardless of timestamps. Portable authored fixtures cover lag,
backlog suppression, extreme timestamp arithmetic and CLI bounds without execution.

## Supplied input delivery age

A 4,096-sample HOST-domain InputDeliveryTelemetry ring is allocated before native
devices. Eligible canonical events actually submitted to Runtime retain their original
metadata; their event point is compared with a fresh same-HOST receipt observation
after acquisition/pop and before dispatch. Excluded pre-origin or other-device events
do not enter this ring. The observer does not apply the input profile offset, clamp
event times or modify judging. Domain mismatch, future event or decreasing receipt
points fail through existing cleanup; event times from different devices may regress
without violating the delivery observer (Runtime chronology remains separate).

Linux reuses the fresh CLOCK_MONOTONIC observation after evdev read, measuring
**kernel-event-to-runtime delivery age** from the preserved evdev timestamp.

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
