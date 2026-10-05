# Native solo source-aware judge construction

NativeJudgeConfig constructs a pristine source-aware judge through the shared
BMS mine-plan helper, preserving its existing asymmetric windows, signed input
offset and ButtonOnly policy. Linux ALSA, Windows WASAPI/optional-ASIO and macOS
CoreAudio solo preparation use this same method before competition identity,
audio activation and optional capture. Native adapters do not introduce their
own mine timing, damage or ordering rules. Existing cancellation and resource
cleanup remain at their original setup boundaries.

Empty mine sources keep legacy configuration/hash behavior. Nonempty mines
enter the actual judge used by source-aware recording and competition identity.
Setup errors return before exposing a partially prepared judge. The durable
contract lives in [BMS mine integration](../kernel/REQ__bms-mines.md).

Four independent deferred groups in `native_mine_fixtures.rs` cover legacy
configuration/record bytes and asymmetric signed offsets, actual SoloRuntime
button ownership and reports, source-aware capture/reconstruction and common
competition identity, and invalid windows/mine grids/caller-default capacity
with disabled-capture early return. These exercise portable common policy and
typed source composition; they do not open native devices or bypass the actual
file preparation guard. No tests have been executed.

After both writers returned actual terminal STOPPED, scoped Rust formatting and
whitespace checks completed. Four locked compile-only checks exited 0:
workspace/all targets with WebTransport, no-default WebTransport/all targets,
WASM browser/lib and WASM browser-audio/lib. Existing WASM cadence unused-code
warnings remain. These checks do not compile Windows/macOS target-specific code
or execute the fixtures.

## Known ceiling

The playable source loader still refuses mine charts. This construction change
does not apply BMS gauge/death, schedule WAV00, render mines or extend completion;
consumer-side live/local/replay/practice/offline integration remains required.
Windows/macOS target-specific compilation and actual native/browser/device
execution remain deferred. No formal review/QA or whole-task completion is
claimed from portable fixture source or compile-only checks.
