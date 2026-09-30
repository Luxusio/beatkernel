# Explicit ASIO recorded BMS output

The existing recorded BMS player now selects optional Windows ASIO by exact
driver CLSID, explicit registry view, mixer-ordered output indices and preferred
or exact buffer frames. It consumes the same checked replay/JudgeEngine audio
plan, actual Mixer and rolling feeder used by the existing host backends, with
no input acquisition or device/rate fallback. A hidden same-thread HWND stays
alive through native teardown; faults/overload terminate with separate command,
Mixer and native diagnostics. Sample `asio-sdk` explicitly forwards SDK
incorporation while default builds remain MIT/SDK-free. Eight CLI fixtures are
authored. Stream snapshots additionally pair successful callback events with
their actual rendered block coherently, retaining post-create driver latencies;
five pure Rust publication fixtures are authored. Locked host/default Windows
GNU/macOS compilation and optional target-only Rust source metadata checks
passed; tests, examples, real SDK/C++/MSVC builds, native playback, independent
reviews and QA remain unexecuted. Live ASIO host/presentation mapping remains
implementation work, and no full runtime completion is claimed.
