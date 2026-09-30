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

All BGM commands are prequeued before Mixer/backend start, with checked
`output_at = song_at + preroll`. BGM count is capped at64,512, leaving1024 live slots;
queue and pending capacities are BGM count+1024 and drain budget covers that queue.
There is no total-note-count limit of4096, implicit voice stealing or streaming claim.
Queue failures preserve exact command/reason and do not undo grading or retry. Active
Mixer voices remain separately bounded; the current ALSA aggregate facade does not
expose all Mixer per-command execution rejection counters, so successful admission
must not be reported as proven audible output. Shared loader asset/path/PCM limits
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
