# Observed WASAPI presentation and song section origin

The Windows runtime must not anchor song time to a wall-clock read after Start.
Use correlated running stream position/QPC observations from the actual fresh
client. Microsoft documents device position as stream-relative, with time given
by position/frequency; native units may differ between modes. Position zero can
persist while initial samples propagate, so zero/Ready observations are rejected.
Only accurate readings are accepted, with two increasing position and host values,
unchanged device frequency and host domain. S_FALSE/degraded readings fail rather
than silently being promoted to a precise measurement.

A portable platform helper converts native position to the mixer's explicit
output-origin grid with checked wide arithmetic, then creates a finite supplied-
pair affine map. The caller declares its validity/extrapolation and uncertainty.
Default uncertainty remains Unknown. The helper can supply an inverse observed
slope; native composition uses that short startup relation only to establish
the frame-zero host origin and starts Transport at normal rate. Extrapolating this origin
is explicit and does not provide an absolute acoustic latency guarantee.

Native section composition preloads bounded WAV PCM before starting output and
selects each cue independently from the original asset. Each repetition opens a
fresh client/Mixer/queue after stop/join of the old stream; old native buffering is
not reused. The chart origin is the applied selected sample position, and first
output frame zero is tied to the observed relation. Shared/exclusive endpoint and
source/output channel compatibility remain explicit; no mode/channel fallback.
Startup waits for usable observations with a finite deadline, while failure still
joins output and closes input registration before window destruction.
Each fresh repetition owns a new [ongoing observer](REQ__presentation-discipline.md)
seeded from actual progressing accurate snapshots. The control thread polls native
observations throughout playback, validates current-host freshness and input host
domains, and updates continuous Transport rate before advancement.
Each pump processes at most 256 native messages before returning to observation
and advancement, so a continuously nonempty message queue cannot starve control.
Past segments remain available for delayed acquisitions. The observer uses the
applied selected song origin, without changing PCM playback rate, chart timing or
judge state.
Unavailable/degraded snapshots may be skipped only within the explicit progress
age; unchanged position does not refresh progress. Terminal stream status, stale
progress, clock resets, frequency/domain changes and excessive rate/phase errors
terminate through cleanup. Measured/correction/applied ppm, phase error and limiting
are reported with Unknown quality. Positive bounded correction does not promise
absolute acoustic synchronization. Observer storage is bounded; Transport history
may grow off-thread during the finite run.
Closing the window cancels all remaining repetitions. The synthetic four-key
chart is anchored at the selected music position; it is not an imported chart.

Native execution, repeatability and loopback measurements remain deferred. Sources:
[Microsoft GetPosition](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclock-getposition)
and [GetFrequency](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclock-getfrequency).
