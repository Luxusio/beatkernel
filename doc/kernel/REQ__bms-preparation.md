# Shared BMS asset preparation

The final BMS sample exposes `PreparedBms`, `ChannelPolicy`, `load_prepared` and
an optional off-thread `AssetDecoder` extension. Preparation owns parsed source,
actual BPM/STOP compilation, an immutable sample bank, head/instant sound
bindings and song-relative BGM commands. It performs no playback, queue
submission, clock mapping or callback work. Callers choose output format and
PCM limits. The default app decoder accepts native FLAC, complete single-stream Ogg/Vorbis bytes or the core's strict RIFF WAVE
PCM16/24/32 and IEEE float32 subset. Explicit WavDecoder and caller codecs remain available;
MP3 and other Ogg codecs/formats still require future implementations or a caller decoder.

Chart text is bounded to the default parser's 8 MiB maximum, is decoded through the shared UTF-8-first/strict-Shift-JIS application policy and
uses default bounded ParseOptions. Decoded UTF-8 separately obeys the same cap. Each encoded referenced asset is bounded to
64 MiB before decoding. PCM per-asset, total-bank and asset-count limits are
caller controlled and also apply after explicit channel expansion. Only WAV
identities referenced by gameplay heads/instant notes or BGM are loaded;
undefined/unsounded long-note tail identities do not trigger loading. Asset
source sample rates remain explicit and unchanged; core mixing performs rate
conversion. The chosen bank output rate is not guessed from the first file.

`Exact` requires every decoded channel count to match the requested output.
`MonoToStereo` additionally permits exactly one source channel to two output
channels, duplicating each amplitude unchanged. It does not downmix, infer a
layout or convert any other mismatch. Expansion length and bytes are checked
before allocation, and allocation failures are returned.

Asset names are resolved against the canonical chart parent. Both slash styles
are treated as separators. Absolute, drive-qualified, parent-component and
canonical symlink escape paths reject. The decoder receives only resolved,
contained paths and already bounded bytes. The containment check is a setup
policy for trusted, non-concurrently-mutating chart directories; it does not
provide a race-free filesystem sandbox against concurrent symlink replacement.

Sound binding construction uses an object-ID index, requiring O(n log n) setup
rather than scanning the compiled chart for every note. Gameplay voice identities
are the complete ObjectId values, not active mixer slot indices. BGM identities
start above the greatest gameplay ObjectId and advance with checked addition.
The loader does not impose the mixer's 4096 active-voice ceiling on total chart
notes or simultaneous queued commands. Callers set and enforce actual runtime
queue, scheduling and active-voice capacities independently. BGM command `at`
values are compiled song timestamps and require explicit playback clock mapping
before native audio admission.

Fixtures author real temporary WAV assets, exact BPM/STOP/BGM/hold timing,
explicit mono expansion, reference-only loading, custom decoding, bounded PCM,
path rejection and more than 4096 chart notes. They are compiled but remain
unexecuted during the user's verification deferral. No native playback, QA,
review or runtime result is claimed by preparation authoring.


## Shared chart character decoding

Application file loaders accept valid UTF-8 first, stripping a single initial UTF-8 BOM; that BOM requires valid UTF-8. Without a BOM, invalid UTF-8 is decoded strictly as WHATWG Shift_JIS (Windows legacy extensions) before the unchanged text parser. UTF-16/32 BOMs, malformed/truncated bytes and replacement decoding reject. Explicit Utf8/ShiftJis modes are available at the shared decoding API; native/GUI/offline/replay loading uses the same Auto policy. This is a deterministic two-encoding preference, not general charset detection; ambiguous bytes that are valid UTF-8 retain that interpretation.

Encoded input and decoded UTF-8 independently obey the parser's 8 MiB cap; output expansion and reader growth fail before parsing. Conversion runs during preparation, with a bounded scratch buffer and capped output allocation, outside audio/input callbacks. Metadata and asset paths preserve decoded Unicode and separator bytes; sound containment and strict parser/resource policies remain in force. Pure literal-byte and real parser/replay composition fixtures are prepared for later execution; build checks do not establish native path/font behavior.


## Native FLAC assets

DefaultAssetDecoder identifies native FLAC by its fLaC data signature; OggS uses the Vorbis policy below, and remaining input uses the existing strict WAV parser, with codec errors surfaced rather than format substitutions. File lookup has the separate exact/compatible policy below. FlacDecoder consumes bounded already-loaded bytes through the app-only pinned Rust claxon library before playback. Source sample rate/channel count are retained; signed integer amplitudes become finite interleaved f32, then the existing exact/mono-stereo channel policy and bank limits apply. Native input/output clocks, judge rules, keysound scheduling and original-time replay are unchanged.

Encoded input is at most 64 MiB; declared sample extents, known sample-count agreement and cumulative decoded storage are checked against the supplied asset cap. Unknown sample counts use actual bounded decoded output. Corrupt/truncated frames and codec errors fail preparation; there is no partial bank success or synthesized silence. Metadata tags/art are skipped, while the codec's frame scratch remains separate from owned PCM and is not represented as an isolated allocation sandbox. Sound heads and explicit BGM alone require assets; unsounded tails remain unloaded. Real synthetic FLAC/PCM/checksum and preparation fixtures are authored for later execution.

Known ceiling: claxon 0.4.3 requires explicit supported bit depth in each FLAC frame header; bit depth inherited through header code zero and 32-bit frame variants return Unsupported. Source STREAMINFO and actual frame rate/channels/depth must agree. No compressed bytes or CRCs are rewritten to hide decoder limitations. Remaining codec conformance is required future work.


## Exact and compatible sample paths

Default shared preparation uses AudioVariants lookup: a literal existing path always wins and must resolve to a contained regular file. Only a genuinely missing literal permits a bounded WAV/FLAC/OGG filename search, recognizing wav/flac/ogg/mp3 suffixes or extensionless references. Supported original extension variants precede the remaining families; otherwise WAV precedes FLAC then OGG. All 32 ASCII case combinations of wav/flac/ogg are considered in deterministic mask order, retaining the exact Unicode stem and directory. Unknown suffixes do not fall back. The parser's original reference and compiled/replay identity never change; the decoder receives the actual resolved path and determines its codec from content.

Exact remains available through AssetPathPolicy and the new full composition entry load_prepared_with_decoder_and_paths. Existing load_prepared_with_decoder retains literal Exact semantics for custom codecs; load_prepared uses the standard decoder with AudioVariants. Corrupt/unsupported existing files do not fall through to alternates. Absolute/parent/drive paths, escaping or dangling links, directory/nonregular targets and non-NotFound errors reject. Every existing candidate independently checks canonical root containment before reading. No recursive/stem casefold search or callback IO occurs. This supersedes the prior default no-extension-substitution rule only at application lookup; the parser still returns opaque exact strings. Static-directory containment is not a race-free concurrent-filesystem sandbox. Fixtures are prepared for later execution.


## Ogg/Vorbis assets

Default preparation shall identify Ogg by OggS content and decode a complete single Vorbis logical stream through app-only pinned Rust lewton. Source sample rate and channel count remain unchanged; interleaved finite f32 enters the same channel expansion and PCM bank limits. Input is bounded to 64 MiB; cumulative decoded bytes are capped before owned output growth, including a preflight from the final EOS granule. Pages require exact framing, version/flags, one serial, consecutive sequence numbers, BOS/EOS and packet continuation consistency, CRC and nondecreasing defined granules. Missing end pages, corruption, trailing data, chained/multiplexed streams and other Ogg codecs reject instead of partial success. Final decoded frame count must equal the final granule for supported zero-origin streams. Application packet decoding shall trim the final audio packet against actual cumulative frames and the EOS granule, including when all audio packets share a single EOS page. Completed-page granules shall agree with cumulative decoded frames; a page granule is not applied prematurely to intermediate packets on that page. It shall not rely on the library high-level reader having observed an earlier audio page. Ordinary codec errors reject. Codec panics that unwind shall become preparation errors without changing global panic hooks; panic-abort builds and allocation/process aborts cannot be recovered by this boundary. Decoder scratch/setup/comment storage is separate from the caller PCM cap; this is not a CPU or total-memory sandbox. The identification packet must be the sole fixed 30-byte BOS packet under the Vorbis mapping, with supported source format checked before setup allocations.

Compatible lookup shall include OGG case variants. Existing literal priority and error refusal stay unchanged; the supported original family comes first, followed by remaining WAV, FLAC, OGG families in that order. Extensionless/MP3 names try WAV, FLAC, OGG. At most 32 ASCII extension-case combinations are considered. Exact custom-codec lookup and original chart/replay identity remain unchanged. Original synthetic Vorbis/Ogg fixtures, limits/container errors and actual default preparation/offline PCM composition shall be authored and compiled for later execution. MP3, other Ogg codecs, chaining/multiplexing, nonzero-origin support and exhaustive codec/native acceptance remain future work.
