# Linux native BMS composition

The separate Linux BMS executable composes shared WAV preparation, actual compiled
chart/rules, canonical evdev input and ALSA output through the existing Runtime.
Native input and output devices, format, periods, buffers and lane bindings are
explicit. Automatic BGM and startup preroll share an output origin, while input
profile offset remains separate. ALSA's native timestamp and checked estimated
sound-frame position provide supplied output/host pairs for the common bounded
presentation observer without fabricated WASAPI metadata. Continuous positive
Transport correction preserves judge chronology and past mappings. Input loss,
stale clocks, resets and native failures stop through output cleanup. This provides
source integration; native playback, physical accuracy, fixture execution and
formal review/QA remain deferred.

Original kernel input timestamps are preserved. A configurable deadline advance
lag defaults to 2 ms, and full input batches postpone deadline advancement until
the queue has been drained. Input remains immediate; the lag delays timeout
notification. Pre-output-origin startup events are counted separately, future
timestamps and older-than-admitted gameplay input fail rather than being clamped.
This finite chronology policy does not bound driver delivery latency.

Rust 1.98.1 locked workspace all-target and Windows-target platform/sample
all-target compile checks passed. Six presentation/conversion fixtures and four
portable sample fixtures were authored and compiled without execution. Native
device permissions, output compatibility, playback and physical timing remain
unverified. The full original Goal remains active.
