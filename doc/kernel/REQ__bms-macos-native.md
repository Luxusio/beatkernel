# Native macOS BMS sample composition

`macos_bms` composes actual supported BMS text/rules/chart, bounded referenced WAV
assets, the same core Runtime/JudgeEngine, scalar IOHIDManager input and exact CoreAudio
PCM output. BGM is automatic; accepted instant/head stages use actual keysounds.
No synthetic gameplay, automatic vendor mapping or GPU interface is supplied.
The shared loader retains its path/asset/PCM bounds; parser warnings are printed.

Required CLI options are `--chart PATH --device AUDIO_DEVICE_ID --keyboard-registry
IOREGISTRY_ENTRY_ID --rate HZ --channels N --buffer-frames N --seconds N`, plus repeated
`--bind CHANNEL_HEX:HID_USAGE_HEX` covering every used BMS lane (11..19/21..29),
including scratch. Native device IDs are positive decimal integers; visible channel
and nonzero keyboard usages are hexadecimal. Duplicate lane/key/options, missing
used lanes and unknown/invalid requests fail explicitly. Buffer is 1..1,048,576 frames,
seconds is 1..3600. Defaults are early/late150000000 ns, signed input offset0,
preroll3000000000 ns (0..10000000000), voices256 (1..4096 concurrent), channel-policy
exact, and deadline advance lag2000000 ns (0..1000000000). `mono-stereo` is an explicit
loader option permitting only mono-to-stereo WAV duplication. Input offset applies
once in JudgeProfile; compiled Judge targets remain unchanged.

The keyboard registry entry must resolve exactly one active button-capable HidDevice.
The caller chooses a keyboard-capable native entry: current descriptor capability
bits do not certify its primary HID keyboard class. Actual canonical HID keyboard
controls alone bind, using `DeviceSelector::Exact` with that attachment's session
DeviceId. Other devices are counted/ignored; unbound controls remain observable in
Runtime binding counters. Selected disconnection or reconnect ends the session and
never retargets a fresh session ID. IOHID permission/native/queue loss errors terminate explicitly. Any unsupported
scalar layout count also ends the session conservatively, including layouts from
unselected acquired devices; there is no invented vendor mapping or guessed release.
Scalar timestamp conversion failures propagate through poll/open errors; the raw-only
report timestamp-failure counter is also checked defensively without claiming it
measures scalar failures. Complete counters are included in loss diagnostics.
Polling belongs to the runloop owner thread, outside audio rendering.

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
No total-note ceiling of4096, streaming
claim or silent voice stealing is introduced. Mixer voices are separately bounded;
queue failures preserve exact command/reason without grading rollback or retry.
CoreAudio exact device/rate/channels/buffer request rejects mismatch/unsupported
native float32 format/layout rather than substituting endpoint or sizes. Applied
settings and explicit source/bindings/profile/options are printed.

Mach absolute/native, normalized HOST and logical OUTPUT are three distinct domains.
The same MachClock instance is used by IOHID and CoreAudio. HidSample metadata already
contains checked normalized HOST time plus native provenance; Runtime receives it in
HOST directly and does not convert it a second time. CoreAudio presentation uses the
[explicit supplied converter](../platform/REQ__coreaudio-presentation.md): callback
first-frame grid and native mach association map to HOST through that same MachClock.
Native output timestamps may legitimately be future relative to callback/control
receipt; they are preserved rather than clamped to a receipt-time sample.

After Start, a two-second bounded seed loop polls actual CoreAudio presentation and
IOHID lifecycle. The first valid supplied pair estimates output-frame-zero host origin
as target minus source elapsed time, with checked wide arithmetic. Transport starts
NORMAL at that estimated origin, song=-preroll. While current HOST precedes this future
origin, judge input/deadlines and correction updates are deferred; pre-origin selected
input is counted/ignored with diagnostics. There is no acoustic accuracy or guaranteed
first-presentation claim: converter/discipline quality remains Unknown.

Every gameplay loop observes actual supplied pairs and validates current-host freshness.
PresentationDiscipline retains bounded progressing observations and continuously updates
only Transport before deadlines; no judge reset/seek, Mixer rate or BGM/preroll remapping
occurs. Defaults retain64 pairs at100 ms intervals, require1 s span, update no more often
than1 s, use10 s phase horizon, max2 s progress age, max250 ms phase and +/-1000 ppm final
rate. Reports print measured/correction/applied signed ppm, phase and limiting. Missing
or unchanged presentation does not fabricate progress/freshness; stale observations,
configuration change or callback failure require explicit cleanup/restart.

The runloop poll is finite and scalar draining is capped256 items per iteration.
A full batch without observing an empty queue defers deadlines while clock observation
continues. Actual input retains native kernel/acquisition time; a future input relative
to a fresh after-pop Mach sample fails explicitly. Deadlines use max(initial origin,
last accepted operation,now-lag), calculated in wide integers. Events older than an
accepted operation fail an explicit HOST chronology check before Runtime processing,
independent of any continuously corrected song mapping, rather than being
clamped/reordered. Equal host timestamps remain permitted for acquisition fanout. Finite lag
assumes bounded delivery delay and delays timeout notification; zero lag can overtake
later-delivered input. It does not adjust grading offset or promise native latency.

Keysounds schedule at the ceiling of the latest successful RenderReport's checked
start_frame+frames on the OUTPUT grid. This core rendered boundary is separate from
presentation and can race a progressing callback; late commands remain visible. Final
native/HID/core RenderReport diagnostics and runtime processing/counters print before
error propagation; None report is explicitly unavailable. Queue admission, successful
core mixing, native buffer delivery and acoustic output remain distinct observations.
See [retained core telemetry](../platform/REQ__coreaudio-render-telemetry.md).

Seconds is the finite gameplay-loop wall duration after seeding, including remaining
preroll/wait for estimated origin. Countdown/current logical song is printed. Startup
can exhaust small/zero preroll; the default supplies logical headroom, not physical sync.
All start/loop failures attempt both audio.stop and input.close before returning; stop
and close errors are reported even when the original operation error wins. Existing
CoreAudio callback quiescence/context retention on native stop failure is preserved.

Portable help and CLI/math fixtures open no devices; non-macOS execution is explicitly
unsupported. Fixtures are authored/compiled but unrun. This lane performs scoped
formatting and host/Apple/Windows compile checks only, no native/link/test execution,
review or QA. Source compilation does not prove playback works on hardware.
