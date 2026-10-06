# WASAPI channel remix and shared native rendering policy

WASAPI adds explicit `open_remixed_recoverable` for channel conversion at an
unchanged source/client rate. Shared/exclusive native setup, buffer leases,
encoding, clock observations and original Mixer recovery remain the existing
paths. The converter is prepared against actual native buffer capacity before
priming and owning the Mixer. Legacy open is strict.

ALSA and WASAPI reuse the portable channel_remix validation/preparation/render
helpers. These helpers make no native calls and use the common core converter;
there is no OS-specific gameplay or channel-matrix algorithm. Portable fixtures
verify actual Mixer/report/pause behavior and exact target PCM16 bytes. Existing
ALSA telemetry is published after successful channel rendering and before
native encoding/submission.

Windows pure preflight fixtures cover both shared modes and exclusive mode.
The old lifecycle fixture is repaired to match original frame-basis ownership
and the worker's Option<Mixer> return type; its synthetic basis is solely for
the existing failure/join test, not device evidence.

See [audio requirements](../kernel/REQ__audio.md). Current verification:

- Host platform tests: 195 passed, 0 failed, 1 ignored, actual exit 0.
- Explicit ALSA null diagnostic after shared-helper migration: 1 passed,
  actual exit 0; native submission/finite-end/original mono Mixer recovery.
- Runtime library with webtransport: 1,574 passed, actual exit 0.
- Workspace all-targets with webtransport and WASM browser library checks:
  actual exits 0, existing dead-code warnings remain.
- Windows GNU platform all-targets source check: actual exit 0. It compiles
  Windows fixtures using the existing C SDK stubs; those fixtures were not run.
- Independent code/security reviews and scoped CLI/library QA: PASS. CLI QA
  depth is test-suite, with separate real ALSA diagnostic execution.

Ignored logs: `target/wf/wasapi-remix-{platform,windows}.log` and
`wasapi-remix-qa-cli-{null,runtime,workspace,wasm}.log`.
Actual Windows COM/shared/exclusive device execution remains environmentally
unavailable: this host is Linux with no Wine/Windows device environment.
The scoped source/portable PASS does not fulfill that acceptance requirement.
No full-task receipt attestation, task verify/close or full Goal completion is
claimed. Cross-platform common tests/type checks do not establish Windows
COM/device/acoustic execution. Rate conversion, CoreAudio/ASIO adapters,
player settings integration and full player acceptance remain unfinished.
