# Connect WASAPI and CoreAudio to the common output replacement controller

Native wrappers retain immutable creation epochs and actual mixer frame bases.
WASAPI delegates recoverable opening/lifecycle and basis-aware tagged snapshot
admission; CoreAudio retains pending partial-stream ownership and uses its
absolute Mach presentation points without adding a second frame offset.
Replacement sequence, timeout and recovery ownership stay in common policy.

## Evidence

Implementation and ten independent tests are authored: six portable actual
snapshot-admission cases, two Windows-only and two macOS-only failure mappings.
Coverage includes stale/max epochs before metadata, soft waiting, terminal
states, duplicate freshness, rational carry/full-width counters and identity
refusal; mappings retain real paused Mixer/queued PCM and original diagnostics.
Portable tests call the same snapshot helper used by
the actual WASAPI bridge; target-only cases construct errors and real Mixer
values without fake native streams or clocks. Assertions/runtime/formal review
and QA remain deferred. Windows/macOS code and target-only fixtures are not
compiled by the four permitted Linux/WASM commands.
Scoped Rustfmt and whitespace checks completed. Four sequential compile-only
checks exited zero: workspace/all-targets WebTransport, headless runtime
all-targets WebTransport, WASM browser lib and WASM browser-audio lib.
The six portable tests compiled in the host workspace check. The two Windows
and two macOS cases, actual adapters and CoreAudio native getter remain
uncompiled by these targets. No assertions or fixture effects ran; existing
unused-code warnings remain. No native/platform acceptance is claimed.

## Known ceiling

Native lifecycle, pending-stream mapping execution, QPC/Mach observations and
device/acoustic acceptance remain unverified. ASIO controller adapter, play-loop
and UI wiring, blocked-call isolation and automatic fallback policy remain
pending. Full BMS player Goal stays active and incomplete.
