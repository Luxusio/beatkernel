# Projection, reverse playback and native section composition

Extended indexed logical projection with Polar and validated caller-defined
projection output. The SVG example consumes radial states and renderer-owned
3D path/pose geometry. Custom callbacks share real Arc ownership and failed
frames clear partial output.

Added bounded reverse replay keysound playback using the existing ReplaySession
and forward judge reconstruction. Normal sample heads, reversed samples and mute
are explicit policies; output time progresses independently of song time. Fresh
Mixer/queue ownership on policy changes isolates pending audio, while the host
still owns native stop/reset and scheduling telemetry.

The Windows runtime example now loads bounded WAV PCM, selects an original
source-frame cue, and opens fresh output for each requested repetition. A pure
WASAPI presentation helper derives the song anchor from accurate increasing
position/QPC observations, with explicit finite validity and uncertainty. Every
input/advance checks validity; window closure cancels remaining repetitions.
No absolute acoustic synchronization guarantee is claimed.

Portable workspace/all-targets and Windows GNU example compile checks passed.
Fixtures were authored for custom/radial projection, reverse PCM/queue chronology,
and observed presentation conversion. Tests, examples, reviews, QA and native
timing measurements remain deferred by the user's instruction. The full plan
and Goal remain incomplete, including ASIO and deferred verification.
