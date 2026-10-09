# BMS video BGA

Video resources declared through BMP references must reach actual native and
browser Scene rendering. Static BMP/PNG/JPEG behavior remains compatible.
Decoder IO and pixels are prepared outside input, audio and rendering callbacks.
The common application layer owns selection; platform adapters own decoding.

## Clock and activation

BeatKernel video policy v1 restarts Base/Layer/Layer2 movies at each exact BGA
marker, including repeated equal image IDs. Activation retains image, channel,
original song timestamp and ordinal. Initial BMP00 Poor is a selection only;
an actually displayed Poor movie starts at the accepted last-miss timestamp.
EOF retains the last eligible frame until the next activation, without looping.
This is an explicit engine policy, not a claim of universal historical-player
compatibility. The [beatoraja event restart implementation](https://github.com/exch-bms2/beatoraja/blob/master/src/bms/player/beatoraja/play/bga/BGAProcessor.java)
is reference evidence; no implementation code is copied.

Movie target is committed original-song time minus activation time. Decoder
arrival time, host time, nominal FPS and an independent media clock do not
advance it. Pausing repeats the target; seeks reproject activation and fence
old decode generations. Native/browser adapters retain one source presentation
origin across seeks. Rational PTS conversion uses checked integer arithmetic.
Variable-rate gaps and composition offsets remain intact. Keyframe preroll
retains the greatest completed presentation frame at or before the target;
discarding every pretarget frame would incorrectly blank gaps.

## Frames, limits and lifecycle

Content/session/decode generation, activation identity, source PTS/time base,
frame revision and RGBA ownership remain explicit. Future frames are not drawn;
late generations cannot change current selection. Decoder ordering or a
presentation watermark establishes completion across reordered output.
Receiver pressure retains transferred pixels and their decoder credits until
admission or retirement. Completion never crosses a still-required rejected
frame. A decoder completion proof can replace an older selected frame atomically
under the existing limit; it cannot evict future or later eligible pixels.
Frame count/bytes/dimensions, encoded input, sessions and lookahead are bounded
and configurable. Backpressure preserves eligible pixels; it never blocks
input/audio/UI waiting for a decoder. An unavailable frame is loading/blank with
an inspectable reason, not a fabricated image or clock.

Stop, reload, seek, suspend, results/disposal and browser Worker retirement
invalidate generation immediately. Native process and pipe cleanup completes
on the IO owner. Browser frames are closed after copy, rejection or retirement.

The native bank retains its service join handle. The game worker joins the
bank before publishing its terminal snapshot and when replacing the chart;
releasing a snapshot on the UI or renderer does not wait for decoding. Browser
decoder retirement fences sessions immediately, and Worker close waits for
in-flight frame copies to settle before terminating the Worker.

## Assets and rendering

Movies are classified before static-image variant lookup. A missing or invalid
movie does not silently fall back to BMP. Canonical aliases share source data;
sessions share only when activation timing matches. Crop/canvas and exact-black
Layer/Layer2 variants are prepared off-thread; Base/Poor retain raw pixels.
Opacity and composition order use existing BGA/Scene behavior.

GPU movie entries retain texture identity at unchanged extent and update pixels
only for a new frame revision through existing Renderer::update_texture.
Extent changes explicitly reallocate. No per-frame texture-bank rebuild or
decoder/file/pixel work is added to the renderer's selection path.

## Concrete adapters and production integration

The native adapter uses an optional user/system-provided FFmpeg executable,
outside the distributed source/binary. Concurrent raw RGBA and metadata reads
pair frame ordinals with integer showinfo PTS and the filter's own time base.
Output duplication/drop is disabled; lost metadata or truncated frames end
that stream explicitly. Optional decoder absence preserves static playback.
No FFmpeg library or executable is bundled by this task; WBS08.18 redistribution
selection remains pending.

Native decoder temporary pixels and retained presentation frames have separate
limits: the decoder working limit defaults to 256 MiB; the frame queue defaults
to three frames, 192 MiB total and 64 MiB per frame. The native movie bank
reserves actual source/transform dimensions against a configurable 512 MiB
aggregate working limit before starting a stream. This is an admission ceiling,
not a startup allocation or a fixed partition between its sixteen slots.
Retiring streams retain their reservation until decoder cleanup completes.

The existing native selection screen displays notes without a BGA surface;
this feature connects movies to the existing gameplay BGA surface. It does not
add a selection-screen video surface. Browser preview retains its existing
playfield surface and uses its committed preview song time.

The browser adapter demuxes real MP4 with pinned MP4Box2.4.1 and its BSD notice,
then uses Worker WebCodecs. DTS determines decode order, CTS and edit lists
determine presentation. Unsupported edit/codec/precision forms are explicit.
Generation-tagged transferable RGBA and bounded independent credits connect
the dedicated decoder Worker to the real Rust Renderer Worker/cache. Window
and gameplay Worker do not decode or render video.

The browser builds immutable presentation/random-access indices during
registration. Target lookup uses binary search and bounded decode spans, and
forward targets within an already supplied frame interval reuse completion
without new decoding or pixel copies. Production demands request the current
predecessor rather than filling every channel with speculative future frames.
EOF means the final presentation was supplied, not merely that decoding reached
the last sample in decode order.

Native registration/background_frames and browser preparation/registration/
frame admission must connect real movie references to Scene. Preview, live,
local, replay/history and results use their existing original-song state.
Standalone queues or mock decoder outputs do not complete video support.

## Verification

Tests cover activation ordinals, repeated IDs, miss time, rational boundaries,
variable-rate/reordered frames, retained preroll, seek generations, EOF,
backpressure, variants/crops/canvas and stable GPU identity. Actual encoded
fixtures must include B-frames, uneven PTS, nonzero origin, keyframe seek and
recognizable colors. Verify native encoded-file-to-Scene and browser MP4-to-
WebCodecs-to-RGBA-to-Scene, alongside existing static BGA behavior.
Document actual OS/codec/backend execution separately from type checks and
physical audio cadence. WBS08.17 stays W until its full acceptance is proven;
the overall player goal remains unchanged.

## Executed evidence — 2026-10-09

Independent DEEP code review, CLI QA and interactive browser QA passed.
The desktop/webtransport app library passed 2,124 tests with zero failures and
six environment-specific cases ignored. The focused model/assets/native tests
passed 51 cases, including explicitly executed VFR/B-frame/nonzero-origin seek
and cancellation fixtures. Two separately executed native Scene fixtures passed,
including Vulkan lavapipe/X11 presentation and captured pixel colors.
The complete Worker/Node suite passed 750 tests. The current browser WASM build
and wasm-bindgen generation passed.

Independent Chromium executed 23 checks: twelve actual MP4/WebCodecs/Rust/GPU
checks and eleven production UI checks covering preview, live keyboard play,
Results, saved history, replay, two-player keyboard/touch and reload. Ten
transferred frames returned ten ACKs; console errors and failed requests were
zero. These are software execution results, not physical audio latency or an
all-OS/all-codec claim. WBS08.17 is D; redistribution/support choice08.18 stays P.

Reproduce the focused Rust cases with `cargo test -p beatkernel-bms-runtime
--features desktop,webtransport --test video_frame_supply --test video_assets
--test video_native --locked -- --include-ignored --test-threads=1`, supplying
explicit `BEATKERNEL_TEST_FFMPEG` and `BEATKERNEL_TEST_FFPROBE`. Native Scene
fixtures are in `app/src/video_native_scene_fixtures.rs`; their module header
and ignored-case reasons specify the X11/Vulkan requirements. Run the Node
suite with `node --experimental-vm-modules --test --test-concurrency=1
app/web/*.test.mjs`; allow more than 120 seconds for its successful paths.
`app/web/video-integration.browser.mjs` documents actual MP4 generation and
browser execution after regenerating WASM. Independent QA logs/screenshots are
under `target/wf/bms-video/qa-cli` and `target/wf/bms-video/qa-browser` and are
ignored; the source fixtures and this evidence summary remain in Git.
