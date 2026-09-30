# Native Windows BMS sample composition

`windows_bms` is a separate binary in `beatkernel-bms-runtime`. The default binary
remains the offline renderer. This native sample loads an actual supported BMS chart
and bounded WAV bank through the shared preparation library, then composes BMS rules,
the same core JudgeEngine/Runtime, real Windows Raw Input keyboard events and WASAPI.
No synthetic hits, tones, automatic gameplay input or GPU UI are supplied. BGM alone
is automatic; accepted head/instant stages publish preloaded keysounds. Parser warnings
are printed, and unsupported parser features remain explicit errors.

Required options are `--chart PATH`, `--device EXACT_ENDPOINT_ID`, `--mode shared|exclusive`,
`--seconds 1..3600`, and repeated `--bind CHANNEL_HEX:HID_USAGE_HEX` for every used visible
BMS lane, including scratch. Channels retain BMS visible identities (11..19/21..29),
with hexadecimal spelling and no inferred game layout. Duplicate channels or key usages,
unsupported channels and missing used lanes fail before playback. Bindings explicitly
use `DeviceSelector::Any`: any acquired physical keyboard can play the chosen keys.
The sample does not infer a specific keyboard identity. The native window must have
focus for foreground keyboard acquisition; console results report actual grades/misses.

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
The initial finite affine interval covers requested loop duration + preroll + three
seconds startup/slack only to construct the origin; ongoing queries are guarded by
fresh presentation discipline, not the lifetime of that initial slope. Startup
observations and ongoing correction still do not establish physical first-presentation
accuracy or repeatable long-run hardware synchronization. Keysounds use coherent
submitted-frame scheduling on their separate output grid with Unknown physical
presentation relation; Mixer reports lateness. `--seconds` is the finite monotonic
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
