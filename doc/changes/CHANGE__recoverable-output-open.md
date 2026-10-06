# Recover software mixer ownership after ALSA open failure

A portable MixerOpenFailure retains the original backend error and optional
unique mixer. ALSA's new open_recoverable path returns unchanged ownership on
preflight and worker-launch failure, and joins startup failures before returning
the worker's original mixer. Existing open preserves its signature and error
behavior. Successful streams still capture the same physical frame basis.

## Evidence

Seven independent tests are authored: two core ownership cases and five actual
ALSA preflight/launch/join cases, including normal error, closed startup receiver
and panic-unavailable paths. Recovered mixers retain advanced paused frame
identity, queued commands and original PCM. Implementation integration is
authored; fixture assertions and threads have not run.
Assertions, fixture threads, hardware playback, formal review and QA remain
deferred. The launch ownership slot is cold: the worker releases its lock and
slot reference before native setup; the control-side empty slot is released as
launch returns. Render processing gains no lock or additional allocation.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
Existing unused-code warnings remain. The seven tests were compiled but not
executed. Native device opening and Windows/macOS/ASIO recovery remain unverified.

## Known ceiling

Panic can lose the Mixer; other backends and automatic handoff remain pending.
Recovery does not undo rendered frames
or prove unheard native buffer delivery. ASIO recoverable open
integration, application live backend transfer, physical fences and failed-open
rollback policy remain pending. Full BMS player Goal remains active.
Subsequent [WASAPI recoverable open](CHANGE__wasapi-recoverable-open.md) extends
the ownership-returning path to WASAPI and shares the cold launch/join owner.
Windows-native lifecycle and device acceptance remain pending.
Subsequent [CoreAudio recoverable open](CHANGE__coreaudio-recoverable-open.md)
retains callback owners whose cleanup refuses and supports retirement retry.
macOS-native lifecycle and device acceptance remain pending.
