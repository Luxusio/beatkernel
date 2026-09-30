# macOS BMS native composition

The macOS BMS executable composes shared chart/WAV preparation, actual compiled
rules, selected IOHID keyboard controls and exact CoreAudio output through the
existing Runtime/JudgeEngine. Device, native registry entry, output format, buffer
and lane bindings are explicit. The selected session identity is preserved;
input loss or disconnect requires cleanup/restart. Callback output frames and
native mach timestamps pass through a checked supplied-mapper converter to the
common presentation discipline without invented WASAPI counters or receipt-time
replacement. Native output time can legitimately refer to future presentation;
startup defers judging until the estimated output origin. Original input times,
preroll/BGM origin, configurable deadline lag and continuous positive correction
remain separate. Cleanup attempts both audio and input teardown and reports actual
core/native counters. Native playback, callback accuracy, tests, linking and
formal review/QA remain deferred; the full original Goal remains active.

Rust 1.98.1 compile-only checks passed after final source changes: locked workspace
all targets on the Linux host, and locked platform/BMS sample all targets for
`x86_64-apple-darwin` and `x86_64-pc-windows-gnu`. The converter's Apple-target
library/test compilation also passed. Portable fixtures were authored and compiled
but not executed. Apple ARM64 compilation, native linking/playback and hardware
input/output evidence were not obtained. No compressed codec support was added;
WAV remains the default preparation decoder with an explicit injected-decoder seam.
