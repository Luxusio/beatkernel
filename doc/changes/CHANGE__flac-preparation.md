# FLAC through shared application PCM preparation

DefaultAssetDecoder accepts native fLaC-signature FLAC bytes and otherwise uses the core's existing strict WAV decoder. Explicit WavDecoder and injected AssetDecoder remain available. Shared load_prepared composes this decoder for native live play, recorded audio and offline rendering, with no OS codec service, callback decoding, filename substitution, source-rate change or new application crate. Referenced heads/explicit BGM alone are loaded; tail and unused assets remain unsounded/unopened.

The app-only pinned claxon 0.4.3 decoder consumes already bounded bytes, skips tags/art, checks declared/actual PCM extents and format continuity, normalizes signed samples into existing finite interleaved f32 and returns the original rate/channels. Existing channel policy and bank/mixer limits apply afterward. Errors reject preparation; no partial decode is replaced with silence. Apache-2.0 text is retained from the exact package, while project-authored source remains MIT and the ASIO distribution policy is unchanged.

Original test-only STREAMINFO/verbatim-frame/CRC fixtures exercise mono/stereo amplitudes, source format, unknown/false sample counts, bounds, corrupt/truncated frames and conflicting format headers. A filesystem preparation fixture loads real synthetic FLAC BGM and WAV hold heads, then feeds the actual shared offline Runtime/Mixer and checks literal PCM, unsounded tails, content-based dispatch and corrupt-asset failure. Fixtures are authored and compiled for later execution; no native/decoder/test acceptance is claimed.

## Known ceiling

Native FLAC is a documented decoder subset; Ogg FLAC, OGG/Vorbis, MP3 and other codecs remain unfinished. Unsupported codec variants return errors. The pinned upstream codec's frame scratch is separate from bounded owned output; this is not an isolated decoder allocation sandbox. MD5 authentication, exhaustive predictor/channel/depth conformance, actual fixture execution, device latency and native playback remain unverified.

Known ceiling: claxon 0.4.3 requires explicit supported bit depth in each FLAC frame header; bit depth inherited through header code zero and 32-bit frame variants return Unsupported. Source STREAMINFO and actual frame rate/channels/depth must agree. No compressed bytes or CRCs are rewritten to hide decoder limitations. Remaining codec conformance is required future work.
