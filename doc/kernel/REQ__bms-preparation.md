# Shared BMS asset preparation

The final BMS sample exposes `PreparedBms`, `ChannelPolicy`, `load_prepared` and
an optional off-thread `AssetDecoder` extension. Preparation owns parsed source,
actual BPM/STOP compilation, an immutable sample bank, head/instant sound
bindings and song-relative BGM commands. It performs no playback, queue
submission, clock mapping or callback work. Callers choose output format and
PCM limits. The default app decoder accepts native FLAC, complete single-stream Ogg/Vorbis, MPEG Layer III bytes or the core's strict RIFF WAVE
PCM16/24/32 and IEEE float32 subset. Explicit WavDecoder and caller codecs remain available;
Other Ogg codecs/formats and remaining codec conformance still require future implementations or a caller decoder.

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

Within one preparation call, equal resolved asset keys reuse the first
successfully decoded and channel-prepared PCM through a fallible owned copy.
Each original reference still resolves through the existing path policy;
reused keys skip another encoded read, decode and channel conversion. The
bounded key-to-first-SampleId map retains no encoded buffers and ends with the
call. Distinct resolved keys decode separately even when their contents match.
This assumes stable resource bytes and decoder behavior during preparation,
consistent with the trusted static filesystem policy; it is not a freshness
check or cross-run cache.

Every SampleId still owns a separate PCM buffer and charges full bank bytes and
sample count. Sound, BGM, voice, judgment and replay identities remain intact.
Only repeated acquisition and conversion work is reduced; PCM storage and
browser transfers are not deduplicated. Source fixtures and compiler checks do
not establish measured preparation speed or actual playback behavior.

Fixtures author real temporary WAV assets, exact BPM/STOP/BGM/hold timing,
explicit mono expansion, reference-only loading, custom decoding, bounded PCM,
path rejection and more than 4096 chart notes. They are compiled but remain
unexecuted during the user's verification deferral. No native playback, QA,
review or runtime result is claimed by preparation authoring.


## Shared chart character decoding

Application file loaders accept valid UTF-8 first, stripping a single initial UTF-8 BOM; that BOM requires valid UTF-8. Without a BOM, invalid UTF-8 is decoded strictly as WHATWG Shift_JIS (Windows legacy extensions) before the unchanged text parser. UTF-16/32 BOMs, malformed/truncated bytes and replacement decoding reject. Explicit Utf8/ShiftJis modes are available at the shared decoding API; native/GUI/offline/replay loading uses the same Auto policy. This is a deterministic two-encoding preference, not general charset detection; ambiguous bytes that are valid UTF-8 retain that interpretation.

Encoded input and decoded UTF-8 independently obey the parser's 8 MiB cap; output expansion and reader growth fail before parsing. Conversion runs during preparation, with a bounded scratch buffer and capped output allocation, outside audio/input callbacks. Metadata and asset paths preserve decoded Unicode and separator bytes; sound containment and strict parser/resource policies remain in force. Pure literal-byte and real parser/replay composition fixtures are prepared for later execution; build checks do not establish native path/font behavior.


## Native FLAC assets

DefaultAssetDecoder identifies native FLAC by its fLaC data signature; OggS uses the Vorbis policy below, ID3/MPEG sync uses the MP3 policy below, and remaining input uses the existing strict WAV parser, with codec errors surfaced rather than format substitutions. File lookup has the separate exact/compatible policy below. FlacDecoder consumes bounded already-loaded bytes through the app-only pinned Rust claxon library before playback. Source sample rate/channel count are retained; signed integer amplitudes become finite interleaved f32, then the existing exact/mono-stereo channel policy and bank limits apply. Native input/output clocks, judge rules, keysound scheduling and original-time replay are unchanged.

Encoded input is at most 64 MiB; declared sample extents, known sample-count agreement and cumulative decoded storage are checked against the supplied asset cap. Unknown sample counts use actual bounded decoded output. Corrupt/truncated frames and codec errors fail preparation; there is no partial bank success or synthesized silence. Metadata tags/art are skipped, while the codec's frame scratch remains separate from owned PCM and is not represented as an isolated allocation sandbox. Sound heads and explicit BGM alone require assets; unsounded tails remain unloaded. Real synthetic FLAC/PCM/checksum and preparation fixtures are authored for later execution.

Known ceiling: claxon 0.4.3 requires explicit supported bit depth in each FLAC frame header; bit depth inherited through header code zero and 32-bit frame variants return Unsupported. Source STREAMINFO and actual frame rate/channels/depth must agree. No compressed bytes or CRCs are rewritten to hide decoder limitations. Remaining codec conformance is required future work.


## Exact and compatible sample paths

Default shared preparation uses AudioVariants lookup: a literal existing path always wins and must resolve to a contained regular file. Only a genuinely missing literal permits a bounded WAV/FLAC/OGG/MP3 filename search, recognizing wav/flac/ogg/mp3 suffixes or extensionless references. Supported original extension variants precede the remaining families; otherwise WAV precedes FLAC then OGG then MP3. All 36 unique ASCII case combinations of wav/flac/ogg/mp3 are considered in deterministic mask order, retaining the exact Unicode stem and directory. Unknown suffixes do not fall back. The parser's original reference and compiled/replay identity never change; the decoder receives the actual resolved path and determines its codec from content.

Exact remains available through AssetPathPolicy and the new full composition entry load_prepared_with_decoder_and_paths. Existing load_prepared_with_decoder retains literal Exact semantics for custom codecs; load_prepared uses the standard decoder with AudioVariants. Corrupt/unsupported existing files do not fall through to alternates. Absolute/parent/drive paths, escaping or dangling links, directory/nonregular targets and non-NotFound errors reject. Every existing candidate independently checks canonical root containment before reading. No recursive/stem casefold search or callback IO occurs. This supersedes the prior default no-extension-substitution rule only at application lookup; the parser still returns opaque exact strings. Static-directory containment is not a race-free concurrent-filesystem sandbox. Fixtures are prepared for later execution.


## Ogg/Vorbis assets

Default preparation shall identify Ogg by OggS content and decode a complete single Vorbis logical stream through app-only pinned Rust lewton. Source sample rate and channel count remain unchanged; interleaved finite f32 enters the same channel expansion and PCM bank limits. Input is bounded to 64 MiB; cumulative decoded bytes are capped before owned output growth, including a preflight from the final EOS granule. Pages require exact framing, version/flags, one serial, consecutive sequence numbers, BOS/EOS and packet continuation consistency, CRC and nondecreasing defined granules. Missing end pages, corruption, trailing data, chained/multiplexed streams and other Ogg codecs reject instead of partial success. Final decoded frame count must equal the final granule for supported zero-origin streams. Application packet decoding shall trim the final audio packet against actual cumulative frames and the EOS granule, including when all audio packets share a single EOS page. Completed-page granules shall agree with cumulative decoded frames; a page granule is not applied prematurely to intermediate packets on that page. It shall not rely on the library high-level reader having observed an earlier audio page. Ordinary codec errors reject. Codec panics that unwind shall become preparation errors without changing global panic hooks; panic-abort builds and allocation/process aborts cannot be recovered by this boundary. Decoder scratch/setup/comment storage is separate from the caller PCM cap; this is not a CPU or total-memory sandbox. The identification packet must be the sole fixed 30-byte BOS packet under the Vorbis mapping, with supported source format checked before setup allocations.

Compatible lookup includes OGG and MP3 case variants with the policy below: the literal path wins; supported original family comes first, then remaining WAV, FLAC, OGG, MP3 families. Extensionless references use that order. At most 36 unique ASCII extension-case combinations are considered. Exact custom-codec lookup and original chart/replay identity remain unchanged. Original synthetic Vorbis/Ogg fixtures, limits/container errors and actual default preparation/offline PCM composition are authored and compiled for later execution. Other Ogg codecs, chaining/multiplexing, nonzero-origin support and exhaustive codec/native acceptance remain future work.


## MPEG Layer III assets and declared timing

Default preparation shall recognize initial ID3 or MPEG sync and decode complete MPEG1/2/2.5 Layer III frames through app-only pinned Rust nanomp3 0.2.0 with scalar allocation-free codec features. Source rate/channels stay fixed within an asset; bitrate and frame padding may vary. It shall reject incomplete frames, skipped/resynchronized junk, unsupported layers/free-format and codec failures instead of manufacturing missing audio. Encoded input is limited to 64 MiB. Complete frame counts and actual retained finite interleaved f32 obey caller asset limits before owned output allocation; fixed codec/frame scratch remains separate. Initial bounded ID3v2 and trailing ID3v1 metadata are skipped, with unsupported malformed tag variants explicitly rejected.

The default tagged-gapless policy shall skip Xing/Info metadata frames and validate declared frame counts against actual following audio frames. Recognized LAME/Lavc delay and padding are validated and applied before PCM admission, including decoder latency. Missing timing metadata implies raw decoded frames; it shall not invent an offset. An explicit raw-frame policy remains available to callers. Unrepresentable trim geometry and unsupported insufficient encoder padding reject under the tagged policy. Exact semantics and remaining tag/CRC/conformance ceilings shall be documented with the implementation; compilation proves no MP3 timing or native acceptance. Codec panics that unwind become preparation errors without changing global hooks; process/allocator/panic-abort termination is not recoverable.

Compatible lookup shall add MP3 while retaining literal priority/error refusal, exact original Unicode stems and custom Exact APIs. Supported original extension family comes first, then remaining WAV, FLAC, OGG, MP3 families in that order; extensionless references use that order. The maximum is 36 unique ASCII extension-case combinations (mp3 has only two letters). Original chart/replay identity is unchanged. Original synthetic MPEG/metadata and actual shared preparation/offline PCM fixtures shall be authored for later execution.

Known ceiling: MP3 metadata skipping accepts one initial ID3v2.3/v2.4 header/body (v2.4 footer must match) and a trailing ID3v1 block; metadata contents are not parsed or validated. ID3v2.2, APEv2, free-format streams, Layers I/II and unknown nonzero Xing encoder extensions are unsupported. Recognized LAME/Lavc extension fields and Xing audio-frame counts are validated, but encoder/tag checksum and optional byte-count/seek fields are not validated. TaggedGapless applies leading delay+529 and trailing padding−529 source frames, requiring padding≥529 and combined trim≤actual raw frames; RawFrames skips the metadata frame but ignores timing cuts. Protected Layer III frames validate CRC16 over header/side information; main data is outside that checksum and broader conformance remains unverified. A clipped beginning with unavailable reservoir returns a codec error instead of dropping frames. Scalar decoding uses fixed decoder and a scratch buffer for 2304 f32 samples, separately from capped retained PCM; timing without metadata cannot be inferred. Native/performance/acoustic acceptance remains pending.

## Invisible keysound source admission

prepare_from_source validates nonempty invisible timing through compile_invisible
after parsing and WAV gain, before replay setup or resource resolution/read/decode.
Source-aware replay validation precedes all asset IO. Load the unique union of
visible-note, compiled BGM and original invisible sample identities, subject to
existing sample-count, path, decode, channel and PCM/bank budgets. All original
selections remain referenced, even when replaced or earlier than a practice
start; unused definitions and inactive/rest-only rows add no PCM. Equal resolved
keys reuse decoding under the existing alias policy. Empty invisible sources
retain ordinary preparation. No silent omission, BGM conversion or fake judged
note. Native/stepped/offline Runtime installation and replay selection use the
shared policy in REQ__bms-input-sounds.md. Test execution and native/browser/
physical audio acceptance remain deferred.
