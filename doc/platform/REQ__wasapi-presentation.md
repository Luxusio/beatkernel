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
Default uncertainty remains Unknown. Transport uses the inverse observed slope
so logical progression follows measured output progression instead of assuming
device/QPC rates are identical. Extrapolating the initial frame-zero host origin
is explicit and does not provide an absolute acoustic latency guarantee.

Native section composition preloads bounded WAV PCM before starting output and
selects each cue independently from the original asset. Each repetition opens a
fresh client/Mixer/queue after stop/join of the old stream; old native buffering is
not reused. The chart origin is the applied selected sample position, and first
output frame zero is tied to the observed relation. Shared/exclusive endpoint and
source/output channel compatibility remain explicit; no mode/channel fallback.
Startup waits for usable observations with a finite deadline, while failure still
joins output and closes input registration before window destruction.
Input and runtime advancement check the finite calibration on every host query;
an expired or mismatched relation terminates through the same cleanup path.
Closing the window cancels all remaining repetitions. The synthetic four-key
chart is anchored at the selected music position; it is not an imported chart.

Native execution, repeatability and loopback measurements remain deferred. Sources:
[Microsoft GetPosition](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclock-getposition)
and [GetFrequency](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclock-getfrequency).
