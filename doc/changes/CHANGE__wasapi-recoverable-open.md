# Retain WASAPI mixer ownership through startup failure

WASAPI opening returns the original backend error and available mixer through
open_recoverable. Its worker retains mixer ownership separately while COM and
native resources are prepared, restores it after prefill refusal, and returns
available ownership on setup, event duplication and startup-channel failure.
ALSA and WASAPI use the same cold static launch/join ownership helpers. Existing
open APIs preserve their signatures and backend error classification.

## Evidence

Seven independent tests are authored: five shared launch/join memory cases and
two Windows-only pure preflight cases. The original ALSA fixture changes only
its refusing-spawner generic trait declaration; its assertions stay intact.
Tuple and optional-mixer worker returns preserve original errors and queued PCM;
None and panic remain explicitly unavailable. Implementation is authored.
Assertions, fixture threads, native devices, formal review and QA remain deferred.
Windows-native constructor/lifecycle branches require later Windows compilation
and device acceptance; portable memory seams do not prove COM cleanup behavior.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The five new portable tests and existing ALSA migration compiled in the host
workspace check. The two Windows-only tests and WASAPI branch did not compile
in these targets. No assertions or fixture threads ran; existing unused-code
warnings remain. There is no Windows lifecycle/device acceptance claim.

## Known ceiling

Panic can lose ownership; Windows lifecycle acceptance and other backends remain pending.
Other backend opener integrations, application
live transfer, native buffer fencing and acoustic timing remain pending. This
retains current software state without guaranteeing pre-priming cursor rollback.
Full BMS player Goal remains active and incomplete.
COM, QPC, prefill/resource cleanup and physical-device recovery remain
unverified. Windows-only tests remain uncompiled by the four permitted targets.
