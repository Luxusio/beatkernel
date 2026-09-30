# Recorded BMS song-time audio

The separate BMS runtime plans keysounds from actual reconstructed JudgeEngine
results, sharing hit-stage selection with live Runtime publication. It removes
the recorded profile offset once before mapping song time onto the explicit
output origin and preroll. BGM is limited to the recorded operation extent,
with stable tied ordering and no sounds for empty logs. The new replay renderer
uses chronological frame groups, finite queue/voice/block capacity and the actual
core Mixer/shared PCM writer. `render_replay_bms` composes bounded WAV loading,
bounded replay reconstruction and newly created raw f32le output; requested frame
cutoff changes audio extent without changing full-log logical counts/hash.

## Known ceiling

Capture did not retain native output scheduling or past audio admission failures,
so original physical sound/delay or dropped audio is not reproduced. Supplied BGM
and PCM are not authenticated by the logical judge fingerprint. Assets and the
command plan are prepared in memory. Native audio replay remains pending. Tests,
examples, native playback, independent reviews and QA remain deferred; source
build checks do not establish sound output or deterministic execution.
