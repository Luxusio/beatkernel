# Common player output channel requests

Adds a pure RemixedOutputRequest and a statically composed RemixedOutputBackend
for the existing gameplay output owner/controller. Strict requests keep the
legacy port; explicit ChannelMatrix requests use an extension port implemented
by ALSA, WASAPI, CoreAudio and optional ASIO adapters. Native adapters only route
to the previously implemented platform methods and preserve their native
recovery/pending-owner diagnostics.

The output and error types remain the original backend types. Epoch/basis,
start/retire, presentation/render reports and original end/pause overrides are
forwarded without interpreting them or introducing alternate lifecycle policy.
Business code can choose a matrix without native I/O; the unchanged owner still
owns pause-boundary replacement and recovery. No dynamic dispatch or new crate.

Tests use actual owner/controller/Mixer memory traces for strict/remixed routing,
epochs, original error and pending cleanup ownership. They do not simulate DSP
as native proof; common PCM conversion is tested in the platform/core suites.
An explicit ignored ALSA null diagnostic exercises the real adapter route.
See [backend requirements](../kernel/REQ__output-backend-switch.md).

Current verification (actual exits 0):

- Full runtime library with webtransport: 1,578 passed, 0 failed, 1 ignored.
  Four composition fixtures cover original epochs/PCM, error identity, pending
  cleanup custody and interval-None forwarding without point fallback.
- Separate actual typed-wrapper/ALSA adapter/null diagnostic: 1 passed. It
  opens the remixed native stream, submits frames, retires and recovers the
  original mono Mixer and basis.
- Workspace all-targets with webtransport and WASM browser checks: passed.
  Existing dead-code warnings remain.
- macOS application library source check: passed using existing SDK stubs.
- Windows application Rust SDK-module source check: passed with an isolated
  target directory and target-only `RUSTFLAGS='--cfg feature="asio-sdk"'`.
  Cargo SDK feature/build scripts were not enabled; no C++ bridge or MSVC link
  was built. [Cargo target flags](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags)
  describe the host/target separation. Normal SDK licensing/build gates are
  unchanged. This diagnostic output is not a valid native SDK build artifact.
- Independent code/security review and scoped CLI/library QA: PASS, with
  real ALSA API execution and no full UI/device acceptance claim.

Ignored evidence: `target/wf/output-channel-fixtures-complete.log`,
`output-channel-native-null.log`, `output-channel-{win-sdk-source,mac-source}.log`
and `output-channel-qa-cli-{lib,alsa,workspace,wasm}.log`.
These results do not establish whole-task receipt attestation, task verify/close
or full Goal completion. Settings text/profile persistence,
initial launch options, rate-conversion clock/buffer integration and actual
Windows/macOS/ASIO device acceptance remain unfinished. This connects typed
replacement composition, not a complete end-user channel settings flow.
