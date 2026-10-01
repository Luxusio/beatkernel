# Synchronized music section restart

Section restart must select the music source frame, reconstruct logical gameplay
at the corresponding song position, and establish a fresh output/host anchor as
one coordinated host operation. Restoring JudgeEngine or changing Transport alone
does not synchronize music. This requirement follows the user's section-playback
sync concern on 2026-09-30.

Select every restart from the original decoded PCM asset and original source song
origin using checked integer arithmetic. Never derive the next restart from the
previous rounded restart position. Frame rounding requires an explicit policy;
report the requested position, selected frame, applied integer song position and
signed correction. Integer nanosecond representation of the selected frame adds
less than one nanosecond; arbitrary requested positions can require up to one
source frame of correction. Source and device sample rates may differ.

Preparing a restart happens off the audio thread. A fresh queue and Mixer own the
selected PCM suffix; they contain no commands or voices from the preceding run.
Existing AudioCommand::Seek retains its documented scheduling semantics and is
not an output-buffer flush. Before installing a prepared restart the host must
stop/join the old stream and reset or replace its native buffered output. It must
reconstruct the judge at the applied song position and replace judge/transport
together. Failed preparation leaves the running session unchanged; native stop,
open and start failures require explicit host recovery rather than an atomicity
claim about external hardware.

The host anchor must correspond to the first selected sample on the new output
timeline through an observed device timestamp and explicit clock mapping. A
wall-clock read after start, or a submitted-frame cursor alone, is not evidence
of physical presentation. Missing mappings, latency and driver uncertainty remain
visible. Pure integer calculations cannot establish an absolute hardware sync
guarantee. Native restart accuracy and repeatability verification remain deferred.

`runtime::restart::RestartPlan` borrows the original PCM to prevent source format
or frame-count substitution between selection and preparation. `prepare_audio`
prepares music-only output owners; `copy_pcm` supports a caller-built bank with
keysounds. `mapped_transport` retains the supplied mapper's reported uncertainty.
`Runtime::replace_session` transfers judge, transport and fresh producer together
and returns old owners for disposal after native stop/reset. These APIs implement
the portable preparation/installation boundary; native presentation calibration
and device buffer reset remain explicit host responsibilities.

`time::AffineClockMapper` can supply the integer output/host relation from paired
observations with a finite validity interval and estimated/unknown uncertainty.
The section example uses labeled synthetic observations to demonstrate the
composition only; it does not collect real device presentation timestamps.

The Windows `windows_runtime` example composes a real fresh WASAPI stream with
original-WAV cue selection. Its observed position/QPC helper anchors the applied
sample position and enforces finite calibration validity, as described in
../platform/REQ__wasapi-presentation.md. It creates a synthetic chart at that
origin rather than restoring an imported recording. This source-level native
composition does not replace pending native timing and restart measurements.

## Graphical retry lifecycle

The BMS graphical player provides a fresh-session retry action. It pins the chart
and native options, validates a fresh invocation before cancellation, waits for
the prior native owner to finish cleanup, and starts with a new publisher and
native output session. Capture filenames derive from the original configured
stem with .retry<N>.bkr. The default starts at the original song beginning;
configured fresh practice starts use the preparation policy below. Historical
play-state restoration still requires the coordinated PCM/judge/anchor operation
above. Each fresh native composition retains its actual presentation calibration,
whose acoustic accuracy and repeatability remain deferred acceptance work.

## Recorded practice sections

Positive practice starts are stored as original song nanoseconds in versioned
replay options. Logical replay reconstructs the same head-filtered chart from
the original BMS; operation timestamps and judge offsets retain their original
meaning. Zero-start recordings retain the existing v1 encoding. Competition
requires identical section starts as well as identical chart and profile.
Offline audio rendering selects overlapping music from original PCM and maps
song time to output time by subtracting the recorded start exactly once before
adding preroll. This restores fresh practice, not historical held-key state.
Native recorded output uses the same preparation. Audio entrypoints require
fresh original assets; the public preparation helper accepts explicit PCM caps,
while app render/native output use 64 MiB per asset, 256 MiB total and 1295
original assets, with the bounded section suffix allowance. Already selected
PCM suffixes must not be passed as original assets. Execution acceptance remains
deferred.

## BMS fresh practice start

A nonnegative --start-ns in graphical native settings begins a fresh practice
session at the original song position and continues to the song end. Start zero
retains full-song behavior. Objects whose heads precede the requested position,
including crossing holds, are excluded from fresh practice; no prior successful
play or held input is invented. Retained target times and tempo/STOP/scroll
markers stay on the original timeline, and the new transport anchors start minus
preroll to observed native output zero. Pristine section judge identities make
recordings section-specific; incompatible full-chart ghosts fail explicitly.

Future BGM cues map original time minus start. Earlier cues still containing PCM
select a suffix from the original asset using explicit ceiling frame selection,
retain the selected frame/correction report, and begin at its original applied
time minus start. Each retained tail owns a distinct sample identity. Expired
cues retire. Fresh bank/queue/output contain no preceding session voices. This
policy adds bounded frame selection and device-grid scheduling corrections;
native physical presentation uncertainty remains visible. It is a fresh practice
start, not restoration of a previously played hold state or a loop boundary.
