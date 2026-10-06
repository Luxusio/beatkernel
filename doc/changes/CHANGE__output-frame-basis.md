# Map fresh device counters onto an existing mixer grid

Core frame-basis values and native ALSA/WASAPI observation paths are
connected so a fresh device counter does not reset an advanced mixer's logical
output position. Stream-local native submission counters remain unchanged;
the original physical mixer frame offset belongs to explicit mapping metadata.

Frame/rate and position/frequency are combined before final nanosecond floor,
retaining fractional carry and avoiding overflowing giant common numerators.
Native stream capture precedes buffer priming. CoreAudio/ASIO already report
absolute mixer frames and must not receive the same offset twice.

## Evidence and known ceiling

Implementation and nine independent core/mapper/stream fixtures are authored.
Windows startup evidence carries the actual basis through initial admission,
seeding and resume; gameplay and replay observations use the same stream basis.
The four core, four platform and one memory ALSA worker cases cover rational
carry, bounds, basis identity, epoch refusal and advanced/paused mixer recovery.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets with WebTransport, headless runtime
all-targets with WebTransport, WASM browser lib and WASM browser-audio lib.
The initial workspace check found an authored mapper expectation using
ClockPoint where the API returns Timestamp; the independent author corrected
that expectation before the successful retry. Existing unused-code warnings
remain. Windows/macOS/ASIO-native branches were not compiled by these targets.
The change preserves zero-offset APIs while introducing checked basis-aware
conversion and stream identity. Device pause/presentation fences, unheard buffer
handling, failed-open rollback, format/rate/capacity changes and live runtime/UI
transition controls remain pending. Assertions, threads, hardware/runtime and
formal review/QA remain deferred. Source/compile compatibility is not acoustic
or gapless acceptance. Full BMS Goal remains active and unproven.
