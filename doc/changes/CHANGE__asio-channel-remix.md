# ASIO explicit same-rate channel remix

The SDK-free AsioBlockRenderer accepts an explicit target format, encoding order
and ChannelMatrix through `new_remixed_recoverable`. It uses the common native
channel preparation/rendering rules, with source/device rate equality, target
scratch and encoding dimensions, original source format introspection and
original Mixer reports/ownership. Legacy construction remains strict.

Optional SDK AsioStream preparation selects target planes from explicit driver
channels and uses the same remixed renderer. Driver rate remains exactly the
source rate. Callback context, priming, original QPC cadence, retirement and
recoverable/pending owner behavior retain their existing paths. No C++ bridge,
SDK acquisition, Cargo dependency or distribution licensing policy changes.

Portable fixtures exercise literal heterogeneous planar bytes, pause and finite
prefix endpoints, invalid-plane no-write/no-progress, setup refusal with intact
source audio, cold mixer transfer and allocation-counted stereo→mono rendering.
See [audio requirements](../kernel/REQ__audio.md).

Current verification (actual exits 0):

- Complete host platform suite: 201 passed, 0 failed, 1 ignored.
- Independent public remixed-renderer fixture run: 3 passed; separate tracked
  stereo→mono allocation/reallocation/deallocation-zero test: 1 passed.
- Runtime library with webtransport: 1,574 passed, 0 failed.
- Workspace all-targets with webtransport and WASM browser library checks:
  passed; existing dead-code warnings remain.
- Independent code/security reviews and scoped CLI/library QA: PASS at
  test-suite depth; no new user CLI or actual driver command is claimed.

The Windows SDK Rust modules were separately source-checked with
`cargo rustc -p beatkernel-platform --lib --target x86_64-pc-windows-gnu --locked -- --cfg 'feature="asio-sdk"' --emit=metadata`.
This enables the final Rust crate's SDK-gated modules only for metadata checking;
it does not request the Cargo SDK feature or compile/link the C++ bridge.
Normal SDK build requirements, MSVC ABI checks and licensed-header gates were
not changed. A successful source check is not a valid SDK build artifact.

Ignored logs: `target/wf/asio-remix-platform.log`,
`asio-remix-sdk-rust-source.log` and
`asio-remix-qa-cli-{focused,allocation,runtime,workspace,wasm}.log`.
Actual Windows/MSVC SDK bridge/ASIO-driver acceptance remains unavailable on
the Linux host and unfulfilled. Scoped results do not establish whole-task
receipt attestation, task verify/close or full Goal completion.
SDK-free tests and Rust source checks do not prove MSVC/C++ bridge builds or
ASIO driver execution. Full-rate conversion, actual player configuration integration and
full Goal acceptance remain unfinished.
