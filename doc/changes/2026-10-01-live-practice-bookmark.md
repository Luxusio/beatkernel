# Live practice bookmark restart

F7/Mark takes the latest accepted nonnegative native song position during live
Play. F8/Restart Mark creates a checked fresh invocation from that exact integer
start through the existing preflight, cancellation, audio/input drain, final
snapshot and joined-owner replacement boundary. The bookmark follows retries
of the session, including Results; F5 restores the original pinned start.
Record retry paths still derive from the original recording base and ordinal.
Replay Watch, loading, missing/negative positions and cancellation cannot mark.

No settings draft is mutated. Failed preflight leaves current playback intact;
explicit cancellation discards prepared replacement and failed cleanup prevents
automatic spawn. A new chart launch starts without a bookmark. The UI uses the
existing button components and exposes both hotkeys and guarded hit controls.

Known ceiling: the mark is the latest coalesced snapshot position, not exact
physical key time. Restart creates a fresh native owner rather than seeking or
pause-resuming a live driver. Loops/live pause, browser/full widget host and
full native/timing/GUI acceptance remain unfinished. No acoustic sync proof is
claimed. Tests/product/GUI/native/GPU/shader/device/file/network/bench and
independent review/security/QA remain user-deferred. Full Goal/task remain open
without acceptance PASS.

Source validation: Linux, Windows GNU and macOS app all-targets, headless
all-targets and WASM graphics library Cargo checks succeeded. Scoped Rust
2024 formatting and diff whitespace checks succeeded. Existing macOS block
future compatibility and WASM cadence warnings remain. Pure fixtures were
authored and compiled only; no tests or product were executed.
