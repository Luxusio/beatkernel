# Integrated runtime contract (Phase 8)

The Windows composition example owns a Raw Input window and WASAPI stream with
an explicit endpoint and shared/exclusive mode. It uses one QPC host origin,
binds four canonical keys and publishes keysounds from judge hits. Scheduling
uses the reported submitted-frame cursor converted to the mixer's zero-origin
output timeline; callback races can make a command late and remain observable.
This is an explicit output scheduling choice, not a calibrated host/device
latency estimate. The finite pump cleans foreground Raw Input messages exactly
once, closes registration before destroying its window, and joins audio before
returning. Software processing percentiles and inferred native counters retain
their distinct labels. Native execution remains deferred.

Optional bounded WAV input and an explicit original-source cue use RestartPlan's
applied frame position as song origin. Each requested repetition starts a fresh
WASAPI client/Mixer/queue after the preceding stream stops and joins. Two accurate
increasing native position/QPC observations anchor output frame zero in the host
domain and preserve the observed inverse rate. Finite mapping validity is checked
for each input/advance; unavailable calibration fails rather than assuming an
origin. Closing the window cancels all remaining repetitions. This four-key
synthetic chart example is composition source, not proof of native timing or a
general file-format gameplay player. Details: ../platform/REQ__wasapi-presentation.md.

The core runtime has one owner and runs outside the audio callback. It composes
canonical physical events, explicit clock normalization, BindingMap, Transport,
JudgeEngine and the bounded scalar audio command producer. The final app owns
native acquisition and output; core has no platform dependency.

Input is mapped to the configured host domain only through a supplied ClockMapper
when domains differ. The earliest original_clock_point and native metadata are
preserved, along with device, physical identity and acquisition sequence. Input
host times and per-device sequences cannot regress; equality permits report
fanout. Transport supplies unoffset song time, and JudgeEngine applies its profile
offset exactly once. Missing mappings and arithmetic/transport errors are explicit.

Every operation supplies an independent audio scheduling ClockPoint. It is mapped
to the configured audio domain before judging; JudgeEvent.at is song time and is
never used as output time. SoundBinding explicitly maps object and result stage to
sample, voice and finite gain. Only hits produce Play. Queue-full/disconnection
returns the exact failed command, increments counters and does not undo judging.
A judge failure in binding fanout stops remaining destinations and is retained in
the report together with all preceding results: fanout is not transactional.

The Phase 8 forward loop does not restore judge state for seek/reverse. Negative
transport segments and song-time regression are rejected. Mutable engine and
transport access are intended for a caller coordinating Phase 10 restoration;
editing the transport alone is not a seek implementation. A forward seek can skip
objects and cause deadline misses; it does not reconstruct historical gameplay.

Timing telemetry retains a bounded ring of processing durations, with nearest-rank
p50/p95/p99/max calculated on demand outside callbacks. These durations measure
software processing only, never physical input-to-audio latency. Native drop and
underrun counts are supplied explicitly by the host. Counters saturate rather than
wrap; unavailable measurements remain absent. Reporting may allocate, and there
is no allocation-free judge/runtime claim. Hardware benchmarks and formal
verification are deferred by the user's instruction on 2026-09-30.

The portable runtime example composes an actual canonical keyboard fixture,
binding, chart judge, scalar queue and Mixer and renders literal PCM. This is
software composition evidence, not native Windows playback evidence.

`Runtime::replace_state` resets chronology only while replacing both JudgeEngine
and Transport by ownership transfer after replay reconstruction. It returns the
previous owners, clears source sequences, and preserves bindings and telemetry.
It does not flush queued audio: the final app must separately synchronize or
replace its audio queue/output when restoring a timeline.
