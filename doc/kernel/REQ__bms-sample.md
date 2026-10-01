# BMS runtime composition sample

The BMS runtime must remain a separate Cargo package/crate from `beatkernel`.
The existing package is `beatkernel-bms-runtime` at `samples/bms-runtime`, with
its own `Cargo.toml`, preparation library and executable composition roots.
`beatkernel` owns reusable game-independent timing, judging, audio and replay;
`beatkernel-bms` owns the BMS parser/rules adapter; `beatkernel-platform` owns
native OS input/output. BMS-specific loading, bindings and application loops
belong to the runtime package. Core and platform must not depend on either BMS
package. Sharing a workspace does not merge these crate boundaries.

`samples/bms-runtime` is a final composition package depending on adapter, core and
platform. Neither adapter nor core depends on platform. Its default offline binary loads an actual UTF-8 or strict Shift-JIS
BMS file through the shared bounded chart decoder and bounded RIFF WAV/native FLAC assets, supplies synthetic button input at compiled note times
through BindingMap/Runtime, schedules BGM and accepted note head sounds, and renders PCM using Mixer and platform encoding. It does not acquire
native input or output through a device and cannot establish native playback or
physical latency. This is an executable composition example, not a game UI.

Asset resolution is relative to the canonical chart parent. Absolute paths,
parent traversal and symlink escapes are rejected. Referenced assets must exist
and be supported strict WAV or native FLAC data decoded during shared app preparation. Source channels must agree; source
sample rates are resampled by Mixer to the explicitly selected output rate.
Files and decoded bank bytes are bounded, and render length is explicitly chosen
by the caller. Output is raw interleaved little-endian float32 PCM created at an
explicit new destination, never silently replacing an existing file. Rendering
uses bounded chunks rather than allocating the entire song output.

The default offline binary accepts `CHART.bms NEW_OUTPUT.f32le SECONDS RATE [CHANNELS]`.
Channels default to two and can be selected explicitly (for example one for mono);
Exact preparation requires matching asset channels. It uses an 8 MiB chart,
64 MiB per WAV/decoded asset and a 256 MiB
decoded bank. Backslashes in asset references are treated as directory separators
for portability. Rendering failure may leave a partial newly created output;
it reports failure rather than treating that file as a completed render.

Shared preparation supplies assets, indexed sound bindings and unique BGM voices.
Synthetic input/BGM admission and software rendering proceed chronologically in
bounded chunks. Total chart notes are independent of the simultaneous voice and
outstanding command limits. Commands and input outside the requested output
interval are not processed. Excess commands at a frame or mixer execution
rejections fail explicitly; synthetic overlapping controls are not promised to
produce perfect judgments. No whole-song PCM allocation is performed.
Formal verification and native playback remain deferred.

The separate `windows_bms` binary composes real Raw Input, the loaded BMS chart
and WASAPI output through the same Runtime/JudgeEngine. It uses a shared bounded
asset preparation library and explicit caller bindings/settings. See
[native composition](REQ__bms-native.md) and [preparation](REQ__bms-preparation.md).
The default binary remains an offline fixture; adding native source does not
establish successful native execution or hardware synchronization.

## Bounded offline rendering library

`offline::render_offline` consumes shared PreparedBms and explicit OfflineOptions
(frames, block_frames, command_capacity, max_voices), writing interleaved float32
little-endian PCM to the supplied Write sink. It does not flush that sink; the CLI
owns create_new and final flush. Frames may be zero, producing no input, grading,
audio render or output. Configuration, total frame/timestamp/byte extent, indexed
synthetic schedule and bounded storage allocation are checked before the first write.
Block frames, commands and voices respect the existing core AudioLimits ceilings.

Synthetic Down/Up events use exact compiled start/end times, stable original note
ordinals, a virtual vendor control for each BMS lane, session DeviceId(1), one explicit
software clock domain and strictly increasing acquisition sequence. Instant notes
emit Down then Up at the same time; holds release at their real tail. Overlapping
lanes/holds retain actual JudgeEngine outcomes rather than promising perfect input.
Judge windows are the existing sample's +/-1 ms with offset zero. OfflineReport.hits
counts Hit stages; judge_results counts all actual JudgeEvents, not completed objects.

Synthetic input and BGM merge by exact timestamp; equal timestamps put BGM before
input and otherwise retain original schedule order. Audio target frames use exact
integer ceiling of timestamp*rate/1e9, matching Mixer. Only one frame group is admitted
before rendering intervening PCM in bounded blocks, so sparse long charts need no
whole-chart queue or note-count voice budget. If one group exceeds queue capacity,
admission fails explicitly; simultaneous voices remain independently bounded.
Runtime's generic enqueue_audio submits BGM unchanged into the same actual Mixer
queue used by judged keysounds, updating common admission counters without judging
BGM or changing clock mappings. Queue failures retain exact commands and reasons;
there is no staging queue, dummy gameplay object or second mixer.

Records whose ceil target frame is at least requested frames are excluded, even if
their timestamp falls between the last rendered frame's start and the unrendered end.
Final timeout processing advances only to the floor nanosecond timestamp at the last
rendered frame's start. Accepted integer input timestamps whose ceil frame is admitted
cannot exceed that final floor, so this preserves host chronology. There is no end-boundary
fake input, whole-chart final grading, or audible claim about an unrendered target.

Each completed Mixer report is checked before PCM encoding/writer output. Any actual
pending/voice capacity, unknown asset/stop, invalid gain/rate/time or unexpected late
command rejects the render explicitly, retaining its typed RenderReport in OfflineError.
Writer/encoding/render failures preserve the most recent successful report when one
exists. Queue admission remains distinct from execution. A failure may leave a prefix
in the caller's writer; already graded state/output is not rolled back or retried.
This library renders no native audio and establishes no physical timing guarantee.

Shared default preparation resolves literal regular assets first, then bounded compatible WAV/FLAC/OGG/MP3 extension variants only if the literal is missing. Original BMS references stay opaque in the adapter. Explicit/custom exact lookup remains available, and containment/read/codec/storage errors reject instead of trying another existing candidate.

Shared preparation decodes complete single-stream Ogg/Vorbis content before native playback, with bounded encoded/PCM storage and source format retained. It rejects container corruption, missing EOS, nonzero-origin frame-count mismatch, chains/multiplexing and unsupported Ogg codecs. Codec/setup scratch is separate from owned PCM limits; tests are authored for later execution.

Shared default MP3 preparation validates complete fixed-source-format MPEG Layer III frames and declared Xing/LAME timing, then admits bounded finite PCM with delay/padding applied. Explicit raw timing preserves decoded frames when requested; missing metadata is not guessed. Original authored MP3/tag fixtures and actual offline Runtime/Mixer comparisons are prepared for later execution.
