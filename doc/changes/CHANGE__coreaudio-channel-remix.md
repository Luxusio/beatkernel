# CoreAudio same-rate channel remix

Adds explicit `CoreAudioStream::open_remixed_recoverable` using the common
ALSA/WASAPI validation, capacity preparation and render helpers. Original source
and requested device rates remain equal; legacy opening remains strict.

CoreAudio retains device layout/ASBD/applied-setting validation. Target channels
size the scratch buffer and native channel-group copying. A portable bounded
copy helper supports interleaved, planar and grouped outputs and rejects invalid
extents before destination writes. The original Mixer report, frame basis,
pause/finite endpoints and native timestamp observations remain unchanged.

The converter lives in the guarded callback RenderState beside its unique
Mixer. It stays alive when native callback/listener retirement fails, and drops
only after confirmed teardown permits original Mixer recovery. No new native
calls, unsafe blocks, dependencies or allocation in the callback are added.

Portable fixtures cover exact group values and no-write invalid extents;
macOS-only pure preflight covers rate/dimension/capacity refusal and unchanged
source frame basis. See [audio requirements](../kernel/REQ__audio.md).
Current verification (actual process exits 0):

- Host platform suite: 197 passed, 0 failed, 1 ignored. Independent focused
  common-helper tests: 4 passed, including channel-group value/no-write cases.
- Runtime library with webtransport: 1,574 passed, 0 failed.
- Explicit ALSA null diagnostic: 1 passed, confirming shared-helper/native
  software-output regression; this does not execute CoreAudio.
- Workspace all-targets with webtransport and WASM browser checks: passed.
- macOS x86_64 platform all-targets source check: passed with existing SDK
  stubs. macOS preflight fixtures compiled but did not execute.
- Independent code/security reviews and scoped CLI/library QA: PASS, with
  QA test-suite depth and explicit native-macOS acceptance limitation.

Ignored logs: `target/wf/coreaudio-remix-{platform,macos}.log` and
`coreaudio-remix-qa-cli-{groups,runtime,workspace,wasm,alsa-null}.log`.
Native macOS HAL/device execution is unavailable on the Linux host and remains
an unmet acceptance requirement. Scoped results do not establish full-task
receipt attestation or full Goal completion; no task verify/close is attempted.
Cross-source checking and portable tests are not HAL/device/acoustic execution.
Full-rate conversion, ASIO adapter,
settings/actual player composition and full Goal acceptance remain unfinished.
