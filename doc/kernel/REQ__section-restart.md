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
