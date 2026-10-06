# Explicit ALSA channel remix

Connects the common core FormatConverter to the actual ALSA render/encode/write
worker through `AlsaStream::open_remixed_recoverable`. Callers supply a validated
ChannelMatrix. Rate equality and source/target channel dimensions are checked
before native acquisition; legacy open remains strict and automatic speaker
mapping is not introduced.

Setup sizes target storage and the converter against the applied native period
before Ready. Rendering publishes the original source RenderReport, remixes into
the device-channel buffer and encodes/submits that buffer through existing
partial-write ownership. Equal rates retain exactly one source frame per device
frame and no resampling history; pause silence, finite prefix endpoints and
OutputFrameBasis remain on their existing frame grid. Stop/join discards the
worker-owned converter and returns the original mixer with its source format.

See [audio requirements](../kernel/REQ__audio.md). Pure production-path fixtures
cover PCM/report/telemetry, pause/finite boundaries, invalid buffers and original
mixer recovery on preflight refusal. An ignored, explicitly invoked ALSA null
plugin diagnostic tests actual native opening, submission and retirement;
it cannot prove acoustic output.

Current verification (actual process exits 0):

- Full platform suite: 193 passed, 0 failed, 1 ignored.
- Explicit ALSA null diagnostic: 1 passed, 0 failed. A real 48kHz mono source
  opens a stereo native stream with buffer256/period64, submits at least one
  native period, reaches the source finite endpoint, stops/joins and returns
  the original mono Mixer with its playback frontier intact.
- Runtime library with webtransport: 1,574 passed, 0 failed.
- Workspace all-targets with webtransport and WASM browser library checks:
  passed; existing dead-code warnings remain.
- Independent code/security reviews: scoped PASS, no findings. Independent
  CLI/library QA: scoped PASS with real native public-API execution through
  the diagnostic fixture; no new user CLI was added.

Ignored evidence logs: `target/wf/alsa-remix-qa-cli-{platform,native-null,runtime,workspace,wasm}.log`.
These results do not establish full-task receipt attestation, physical device
presentation/drain quality or full Goal acceptance. No task verify/close is
attempted for the unfinished player task.

Sample-rate conversion, other native adapters, live replacement/UI options and
physical-device timing remain unfinished. This is one native I/O adapter using
shared core conversion rules, not platform-specific gameplay policy.
