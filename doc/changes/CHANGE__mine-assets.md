# Common WAV00 audio assets

The existing shared audio resource union and bank loading loop use an internal
reusable component called by prepare_from_source.
The selector includes original SampleId(0) only when a defined WAV00 has an
actual nonfatal mine, using the common validated mine plan. Existing visible,
BGM and invisible assets retain their original IDs and preparation policies.

The same scoped source/decoder path handles optional WAV00, including bounded
encoded reads, shared resolved-key decode reuse, separately owned sample PCM,
bank limits and explicit channel policy. High-level mine admission still fails
immediately after parsing, before gain, replay or resource work. Completing a
typed PCM loader does not enable playable mine files.

Five independently authored deferred fixture groups cover reference selection
and capacity, actual MemoryFiles/WavDecoder lookup and independent alias PCM,
loaded WAV00 used by actual Runtime/Mixer, loader failures and channel/bank
policies, and high-level guard order with existing ordinary preparation.
Encoded read bounds and controlled read refusal are covered in authored code;
a real returned buffer exceeding 64 MiB was not constructed.

Both writers delivered terminal Writes STOPPED before scoped rustfmt. Four
compile-only checks completed with exit 0: workspace all targets with
WebTransport, headless all targets with WebTransport, WASM browser and WASM
browser-audio. git diff --check completed successfully. Existing unused
playfield-wrapper and WASM cadence warnings remain. Assertions were not run.

Gauge/fatal-stop policy and final file admission are unfinished. Assertions,
applications, devices, browsers, performance acceptance and formal review/QA
remain deferred; the full Goal is active.
