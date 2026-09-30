# Shared BMS asset preparation

The final BMS sample exposes `PreparedBms`, `ChannelPolicy`, `load_prepared` and
an optional off-thread `AssetDecoder` extension. Preparation owns parsed source,
actual BPM/STOP compilation, an immutable sample bank, head/instant sound
bindings and song-relative BGM commands. It performs no playback, queue
submission, clock mapping or callback work. Callers choose output format and
PCM limits. The default decoder supports the core's strict RIFF WAVE
PCM16/24/32 and IEEE float32 subset; other codecs require a caller decoder.

Chart text is bounded to the default parser's 8 MiB maximum, is valid UTF-8 and
uses default bounded ParseOptions. Each encoded referenced asset is bounded to
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
