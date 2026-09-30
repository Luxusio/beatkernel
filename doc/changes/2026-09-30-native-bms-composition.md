# Native BMS composition and bounded preparation

Added a shared final-sample preparation library with explicit output format,
bounded chart/referenced-asset loading, canonical relative path validation,
head/instant bindings and separate BGM commands. WAV remains the default decoder;
an injected off-thread AssetDecoder provides extension without changing judging
or output. Exact channels or explicit mono-to-stereo duplication preserve source
frames/rates. Unique voice identities do not cap total notes at concurrent voices.

Added a separate Windows-native binary combining an actual supported BMS chart,
real Raw Input keyboard events, Runtime/JudgeEngine and WASAPI. Endpoint, lane/HID
bindings, shared/exclusive mode, buffer/period, profile offset and capacity are
explicit. BGM is prequeued; keysounds follow admitted real judge hits. Startup
preroll shifts BGM and transport coherently while leaving chart timing unchanged.
Two observed device/QPC pairs supply a finite clock relation with unknown drift
error. This does not establish long-run rate stability or physical synchronization.

Portable workspace and Windows sample all-target compile checks passed.
The existing offline binary remains the default command. Source and preparation
fixtures are authored for later verification; test/example/native execution,
formal review and QA remain deferred. The full original Goal remains active and
incomplete, including ASIO and required verification evidence.
