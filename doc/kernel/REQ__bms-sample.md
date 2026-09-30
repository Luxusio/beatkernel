# BMS runtime composition sample

`samples/bms-runtime` is a final composition package depending on adapter, core and
platform. Neither adapter nor core depends on platform. Its default offline binary loads an actual UTF-8
BMS file and bounded RIFF WAV assets, supplies explicitly synthetic perfect
button input through BindingMap/Runtime, schedules BGM and accepted note head
sounds, and renders PCM using Mixer and platform encoding. It does not acquire
native input or output through a device and cannot establish native playback or
physical latency. This is an executable composition example, not a game UI.

Asset resolution is relative to the canonical chart parent. Absolute paths,
parent traversal and symlink escapes are rejected. Referenced assets must exist
and be WAV PCM supported by the core decoder. Source channels must agree; source
sample rates are resampled by Mixer to the explicitly selected output rate.
Files and decoded bank bytes are bounded, and render length is explicitly chosen
by the caller. Output is raw interleaved little-endian float32 PCM created at an
explicit new destination, never silently replacing an existing file. Rendering
uses bounded chunks rather than allocating the entire song output.

The default offline binary accepts `CHART.bms NEW_OUTPUT.f32le SECONDS RATE`, up to 4096
scheduled sounds, an 8 MiB chart, 64 MiB per WAV/decoded asset and a 256 MiB
decoded bank. Backslashes in asset references are treated as directory separators
for portability. Rendering failure may leave a partial newly created output;
it reports failure rather than treating that file as a completed render.

Synthetic event generation and judging happen before software rendering. Queue
and pending capacities bound the command count; excess input fails explicitly.
Formal verification and native playback remain deferred.

The separate `windows_bms` binary composes real Raw Input, the loaded BMS chart
and WASAPI output through the same Runtime/JudgeEngine. It uses a shared bounded
asset preparation library and explicit caller bindings/settings. See
[native composition](REQ__bms-native.md) and [preparation](REQ__bms-preparation.md).
The default binary remains an offline fixture; adding native source does not
establish successful native execution or hardware synchronization.
