# Native ALSA timing observations

ALSA output now provides a separate coherent timing snapshot built from a
preallocated worker-owned native status container. Native state, signed delay,
available frames, raw high-resolution timestamp, submitted count and monotonic
query bracket are retained together. Timestamp enable/type configuration is
explicitly applied and checked; unsupported capabilities fail without a clock
fallback. Running state and valid timestamp/delay permit an estimated sound-frame
position, while unavailable queries and terminal output invalidate publication.
Native timestamps are not replaced with userspace receipt time. Existing aggregate
counters remain independently observed. The native example prints timing before
stop and its unavailability after stop. These additions do not prove acoustic
latency or implement a Linux-native BMS composition. Fixture execution, native
playback, independent review and QA remain deferred.

Rust 1.98.1 locked workspace all-target and Windows-target platform/sample
all-target compile checks passed. Six pure fixtures were authored and compiled,
with no execution results claimed. The full original Goal remains active.
