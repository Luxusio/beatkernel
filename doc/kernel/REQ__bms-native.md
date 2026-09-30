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

All BGM commands are prequeued before Mixer/backend startup, with checked
`output_timestamp = original_song_timestamp + preroll` on a fresh output grid
at zero. Compiled chart/Judge targets stay unchanged. BGM count is bounded at 64,512;
queue and pending capacities are BGM count + 1,024 reserved live slots (at most 65,536).
The render drain budget covers the full prefilled queue; the native backend still owns
bounded buffer rendering. Concurrent voice/pending/rate execution rejections and late
commands remain visible in audio snapshot counters. Queue failures report exact commands
without retry or judge rollback. No 4096-total-notes ceiling or silent voice stealing
is introduced. Shared loader file/path/PCM limits apply (64 MiB per asset, 256 MiB bank).

After native Start, two coherent accurate WASAPI device-position/QPC observations establish
`WasapiPresentationClock`. The observed relation maps output frame zero to logical song
`-preroll`; input and deadline queries explicitly check the finite calibrated
validity interval. The unit song/output relation makes prequeued BGM output times
exactly song time plus preroll; observed inverse slope applies only between host
acquisition and song.
Two startup observations infer a slope over a short span; they do not establish
rate stability over the whole duration or repeatable native synchronization.
Recalibration and physical measurement remain future work. Default three-second
preroll gives up to two seconds for calibration before song zero; smaller preroll
can expire during calibration. Zero preroll explicitly permits startup calibration
to advance initial BGM/notes before input processing begins. Progress prints
remaining countdown or current song nanoseconds, with a window focus prompt;
this is logical composition, not a physical first-presentation guarantee.
Calibration waits at most two seconds; failure ends the session rather than substituting
receipt time. Checked validity extends from output zero through requested loop
duration + preroll + three seconds of startup/slack,
with explicit bounded extrapolation and caller-supplied 100 ns observation representation
error but no supplied drift bound. Relation quality therefore stays Unknown; physical
first-presentation accuracy, DAC latency and keyboard-to-speaker latency remain unmeasured.
Keysounds schedule from coherent submitted-frame telemetry with the independent output
domain and Unknown relation to physical presentation; they may be late, reported by Mixer.
`--seconds` is the finite monotonic wall duration of the gameplay loop *after*
calibration, including any remaining preroll countdown. The gameplay pump begins after calibration; any queued native messages are
acquired through the real Raw Input path with its retained timestamps rather than
synthesized. Prefilling BGM prevents its startup commands from being lost.

All setup/start/calibration/pump exits stop and join the stream and release Raw Input before
native window destruction. Native resources use RAII guards for early failures. Explicit
cleanup errors remain errors; if unregister repeatedly fails, the stateless window/class
is retained rather than destroying a still-registered target. No game input is invented on
focus loss or unplug; applications requiring held-state cancellation need explicit policy.

`--help` and argument validation are portable. A non-Windows native request returns an
explicit unsupported-host error. This lane authors source and uses formatting plus host/
Windows compile checks only; execution, native delivery, tests, reviews and QA are deferred. Portable preroll
CLI boundary, BGM shift/identity and checked overflow fixtures are authored inside
the binary under cfg(test), compiled but not executed.
