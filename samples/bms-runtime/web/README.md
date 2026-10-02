# Browser BMS player

The optional `browser` feature belongs to the existing `beatkernel-bms-runtime`
application crate. This host imports user-selected files, prepares chart/audio/
image data through the common Rust algorithms, and displays compiled notes and
static BGA at an explicit song position. The Play source path also transfers
prepared PCM to an AudioWorklet and feeds originating Window keyboard timestamps
into the same SoloRuntime used by native gameplay. The natural-completion source
path joins terminal judging, Mixer drain and reported output timestamps.
Optional capture/download and local canonical replay playback are source-wired
to this host. Browser multiplayer remains unfinished.

The DOM owns file selection, controls and layout. A dedicated module Worker owns
the imported bytes, preparation and wgpu rendering on a transferred
OffscreenCanvas. The host uses plain JavaScript without React or external
scripts. Existing retained note instances and BGA texture caching are reused.

## Build and serve

Run the following from the repository root when browser execution is scheduled.
These build, binding-generation and server commands have not been executed as
part of the current source-only development phase.

Use Rust 1.98.1 or newer with the WASM target installed:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser --release --locked
```

Generate the web bindings with `wasm-bindgen-cli` **0.2.129**, matching the pinned
Rust dependency:

```sh
cargo install wasm-bindgen-cli --version 0.2.129 --locked
wasm-bindgen --target web --out-dir samples/bms-runtime/web/pkg target/wasm32-unknown-unknown/release/beatkernel_bms_runtime.wasm
```

The generated `pkg/` directory is a local build artifact and is not committed.
The static host loads its generated JavaScript and WASM from that directory.
Serve the directory over localhost, for example:

```sh
python3 -m http.server 8080 --bind 127.0.0.1 --directory samples/bms-runtime/web
```

Open `http://127.0.0.1:8080/`. Use HTTPS when hosting elsewhere. The host requires
a secure context, module Workers, transferable OffscreenCanvas and WebGPU in a
dedicated Worker. Opening the page through `file://` is unsupported. The pinned
wgpu configuration provides WebGPU; this host has no WebGL fallback. Unsupported
capabilities and preparation failures are reported explicitly.

## Select and preview

1. Select a chart folder, including its audio and image resources. Folder
   selection preserves relative paths. The flat-files option is useful for
   charts whose resource names match the selected filenames; it cannot
   reconstruct missing subdirectories.
2. Choose a chart from the imported `.bms`, `.bme`, `.bml` or `.pms` list. Set
   the preparation sample rate and unsigned 64-bit random seed, then prepare
   the chart. The mix rate starts at 48,000 Hz. Individual assets retain their
   original sample rates, and mixed source rates use the existing kernel policy.
   Preparation adds no pre-resampling pass. This mix format is not an actual
   browser audio device selection. The supplied host
   prepares stereo output and permits the common mono-to-stereo channel policy.
3. Enter a nonnegative original-song time in decimal seconds, with up to nine
   fractional digits, and request a preview. For example, `12.345678901` selects
   exactly 12,345,678,901 nanoseconds. The boundary uses signed 64-bit nanoseconds
   through BigInt and reserves room for the fixed two-second note lookahead.

The displayed position changes only when requested. Animation callbacks schedule
redraws; their timestamps do not advance the song. No physical keyboard press or
miss feedback is fabricated for this preview. BGA selections, opacity and note
positions come from the actual prepared chart.

## Resource limits and lifecycle

The supplied host admits at most 32,768 selected files, 64 MiB per encoded file,
256 MiB total encoded bytes and 4,096 UTF-8 bytes per normalized relative path.
Metadata is checked before sequential file reads, and Rust checks admission
again. PCM preparation has a separate 64 MiB per-asset and 256 MiB aggregate
budget; decoded images have their own limits. These are not an overall browser
memory cap: binding copies, decoder scratch, prepared data, GPU resources and
overlapping old/new imports consume additional memory.

Resources resolve relative to the chart's selected parent directory. Unsafe
paths, normalized duplicates and file/directory collisions are rejected. Charts
cannot request unselected files or turn resource names into network URLs.

Imports and asynchronous initialization carry generation identities. A stale
result cannot replace a newer selection, and Worker mutations are serialized.
One import pump admits one active read/candidate and only the latest queued
selection. A superseded file read settles before that candidate is freed and
the queued import starts. Relative paths travel as explicit message fields.
The Worker stages a completed import until the DOM accepts its current catalog
generation. Ignoring an old result or failing the next import therefore keeps
the same accepted library on both sides.
Rendering is event driven with bounded surface retries. Page teardown terminates
the Worker and releases presentation callbacks and layout observers; restoring
the page creates a fresh canvas owner and requires selecting files again.

See the [browser contract](../../../doc/kernel/REQ__bms-browser.md) for the full
boundary. Portable fixtures are authored for the shared source/preparation path.
The deferred JavaScript regressions use Node's built-in test runner and VM module
mocks of WASM ownership. They cover the actual host helpers and Worker without
requiring a GPU or generated bindings. Run later from the repository root:

```sh
node --experimental-vm-modules --test samples/bms-runtime/web/host_model.test.mjs samples/bms-runtime/web/worker.test.mjs
```

This command has not been executed. It does not replace real DOM/Worker/WebGPU
acceptance. Current host workspace, headless application, WASM browser and
WASM browser-audio source configurations compile with Rust 1.98.1. Historical
Windows/macOS checks do not establish current native QUIC cross compilation. Tests, binding
generation, browser/Worker/GPU execution, audio/timing acceptance and formal
review/QA remain deferred; the full player task stays open.

## AudioWorklet component

The separate `browser-audio` feature exports the existing Rust Mixer through a
numeric ABI for `audio-worklet.js`. It belongs to the same application crate
and excludes graphics. The Play controls connect the component to common gameplay and keyboard input.
Optional capture/download and replay use the same host; browser network play
remains separate work.

When execution is scheduled, build and generate this artifact separately from
the graphics bindings above. Each Cargo build replaces the common output WASM,
so generate each feature's bindings immediately after its own build:

```sh
cargo build -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser-audio --release --locked
wasm-bindgen --target web --out-dir samples/bms-runtime/web/audio-pkg target/wasm32-unknown-unknown/release/beatkernel_bms_runtime.wasm
```

`audio-pkg/` is ignored generated output. These commands have not been executed.
The host compiles its WASM outside the callback, provides the compiled module
to the processor and activates AudioContext from the Play user gesture.
The processor's preparation and control protocol is documented in its source.
It imports the UTF-8 compatibility bootstrap before generated bindings, because
encoding constructors may be absent in an AudioWorklet global.

The Worklet owns its own PCM bank, queue and Mixer in a separate WASM memory.
Setup allocates bounded storage and a fixed interleaved output view. Source
sample rates are retained. The one-shot start uses absolute AudioContext frames;
the first suffix after that boundary is relative Mixer frame zero. Actual block
lengths are used. Gaps, regressions and unexpected memory growth terminate the
owner instead of inventing render progress. Control acknowledgements distinguish
queue admission from actual rendered commands. Polling returns genuine Mixer
reports outside the callback; those reports are not output-presentation or
acoustic timing evidence.

`audio-host.mjs` adds the actual AudioContext/AudioWorkletNode owner. It is a
component called by the player page. Prepare a `WebAssembly.Module` for the audio artifact before the user gesture,
then call `AudioHost.open` from that gesture with explicit generation, channels,
PCM limits and audio limits. Opening requests resume immediately and waits for
the exact Worklet readiness response within one setup deadline. An optional
AbortSignal cancels setup. Read-only `sampleRate` reports the actual context rate
for subsequent chart preparation.

Resource setup calls `sample({id, rate, channels, pcm})` sequentially, then
`finish()`. Each PCM array must own its entire standalone backing buffer;
posting consumes that buffer. `commands(batch)` preserves the Worklet's exact
numeric command records; `arm(absoluteFrame)` selects the one-shot context start.
The read-only `currentFrame` is a BigInt estimate derived from context time for
choosing a future start target. The Worklet rejects requests that arrive too late;
this estimate does not measure presentation or acoustic output. A context that
stops running fences the owner rather than silently continuing its session.
One ordinary operation may be pending. Calls that overlap reject locally, and
the host does not create an unbounded control queue. Worklet rejection carries
the exact admitted prefix in `AudioHostError`; game operations must never retry
that batch. `poll()` returns genuine Worklet report words, and `stop()` shares
one bounded cleanup operation that disconnects and closes the audio owner even
if the stop acknowledgement fails.
A close timeout reports failure and cannot establish browser resource release.

The host regressions are authored for Node's VM runner with mocked WebAudio
globals. They have not been executed and do not establish actual browser output:

```sh
node --experimental-vm-modules --test samples/bms-runtime/web/audio-host.test.mjs
```

Prepared-resource wiring and the shared nonblocking gameplay owner are authored
and compile on the host/headless/browser source configurations. Behavioral
acceptance has not been executed.
Browser device buffering, JavaScript/GC and MessagePort do not provide a hard
realtime guarantee or native WASAPI/ASIO controls. Generated bindings, Worklet
execution, browser output and behavioral tests remain deferred.


## Play source path

Build both `pkg/` and `audio-pkg/` as described above, generating each package
immediately after its own feature build. Prepare a preview, then choose **Play
from beginning**. Playback re-prepares the selected chart at the actual context
rate; the preview position does not act as a playback seek. PCM transfer and
initial BGM queue admission complete before selecting a future one-shot start.

The page shows the actual lane bindings. The first keyboard side uses left
Shift for scratch and Z/S/X/D/C/F/V for seven-key lanes; Space covers the
additional lane. The second uses right Shift and N/J/M/K/Comma/L/Period, with
Slash for its additional lane. DOM keyboard events provide one logical device.
Stop, Escape, focus loss and page hiding cancel the session. Controls remain
locked until audio and Worker ownership are released; ordinary stop restores
the accepted preview and reports the available actual score.

Input uses original Window event timestamps and bounded FIFO steps. Graphics
animation timestamps never advance the song. Runtime processing and audio
command admission continue independently against bounded queues. Actual Mixer
reports credit rolling BGM. Actual output/performance timestamp pairs feed the
same bounded presentation observer as native playback. Correction changes the
input transport rate continuously after a complete accepted input watermark;
original event timestamps and historical judgments remain unchanged. The
nominal start projection and browser estimates are not measured acoustic latency
or an accuracy guarantee. Optional shared capture and replay download are
source-integrated. Portable stepped replay and WASM preparation/render bindings
reuse canonical recorded work and existing audio planning. Window/Worker replay
launch reuses the same audio host. Explicit saved-record storage/browsing feeds
the replay path; browser competition and networking remain unfinished.

Natural completion uses the existing shared SongCompletion owner. Every original
object must finish judging; BGM and outgoing/local command work must finish;
then a subsequent idle Mixer block must be covered by a genuine reported output
timestamp. The Window joins its captured input and outstanding operations before
closing the game/audio owner and showing the actual final score. A duration or
last-note timer never substitutes for output drain.

The host forwards output timestamps at their reported context position, with no
extrapolation by elapsed UI time. It accepts points at most one second old and
subtracts the exact armed start conservatively. Zero, stale, future, regressing
and prestart evidence cannot complete playback. Missing output timestamp support
keeps manual Stop available. Malformed or suspended-owner evidence fails the
session. This browser-reported estimate is not acoustic latency validation.
The observer retains at most 64 pairs, spaced by at least 100 ms, warms up over
at least one second and updates no more often than once per second. Its native
default bounds are 1,000 ppm rate deviation, 250 ms phase error and a ten-second
phase correction horizon. These are initial policy bounds, not device accuracy.
Missing or stale observations skip correction and hold the current transport
rate; duplicate output does not refresh evidence. Coarse host timestamps without
host progress defer admission. Excessive rate/phase error or malformed clock
relations fail explicitly rather than rewriting committed input.

Additional deferred regressions are authored for the portable Rust owner,
Worker adapter and shared numeric helpers. Execute only when the deferred test
phase is resumed:

```sh
node --experimental-vm-modules --test samples/bms-runtime/web/play-model.test.mjs samples/bms-runtime/web/play-worker.test.mjs samples/bms-runtime/web/play-host.test.mjs samples/bms-runtime/web/audio-host.test.mjs samples/bms-runtime/web/worker.test.mjs
```

No JS assertions, browser runtime, generated bindings or audio output have been
executed in this phase. Compilation and fixture authoring are not player QA.

## Optional replay recording

Enable **Record replay** before Play to record the actual shared Runtime reports.
It is disabled by default. Actual prepared branch seed, input provenance and
accepted song times use the existing native replay metadata and codec. Clock
correction is reflected in accepted song times; input timestamps are preserved.
The initial limit is 64 MiB of encoded data and 1,000,000 accepted operations.
Reaching a capture limit stops the run with its available prior prefix and actual
score. These are recording limits, not the core song-time range, and disabled
recording adds no recording duration limit. Allocations/copies can exceed the
encoded-data limit in aggregate; capture runs on the control owner, never the
audio callback.

After game and audio cleanup finish, **Download last replay** exports the last
recorded result. It is labeled complete only after genuine natural completion
and successful cleanup; manual stop, cancellation and failure produce a prefix.
That label is in the UI/filename, not a new trusted field in the canonical file.
Native replay tools can decode the existing format using the matching chart
setup; the setup fingerprint is noncryptographic and excludes device/audio-file
identity. A renamed file does not prove complete playback.

Only one result is retained. Downloads are explicit; no file is automatically
written or uploaded. Blob URLs are created only on a download click, revoked on
replacement/page hiding and after at most 60 seconds. Stopped capture extraction
is a single encoding attempt; serialization failure remains explicit and does
not hide game cleanup failure. Local replay import/playback is source-integrated;
Explicit saved-record storage/browsing feeds that replay path; browser
competition and network opponents remain follow-on work. Source checks and authored fixtures
do not establish that a browser download or replay ran.

## Replay runtime components

The same application crate now exposes `StepReplay`, a nonblocking owner using
the existing `ReplayVisual`, replay audio planner, rolling feeder and
`ReplayCompletion`. It holds one bounded original command batch until ACK and
shares output-report and admission validation with `StepGameplay`. Actual
presentation advances recorded operations; command credit comes from actual
Mixer reports. Missing presentation does not advance judgments. A prefix ends
at its recorded operations and never synthesizes chart completion or later BGM.

`BrowserLibrary.prepare_replay_chart` decodes a bounded canonical recording,
uses its recorded branch seed and selects its original section. `BrowserReplay`
consumes that prepared resource once, exposes the same sample/command ABI and
accepts actual output reports. `BrowserView.draw_replay` uses the common canvas
with actual score, note progress and canonical eighteen-lane pressed state.
The Window/Worker host now uses these components for local recorded playback.
Source checks establish compilation only; fixtures and browser/audio execution
remain deferred.

## Play a local replay

Select the matching song folder and prepare its chart, then choose one `.bkr`
file under **Local replay** and click **Play replay**. The file must be nonempty
and no larger than 64 MiB. Selection retains metadata; the Worker checks it
again before reading once and rejects a changed-size or cancelled read before
WASM construction. The canonical recording supplies its branch seed and section;
the live seed control does not override it. Selected files stay on the device.

Replay prepares samples at the browser's actual output rate and uses the same
AudioHost/Worklet, sample transfer, original command batches, exact armed start,
ACKs and joined cleanup as live play. Keyboard events do not judge replay;
Escape, Stop, losing focus or hiding/leaving the page stop it. The display shows
recorded score and pressed lanes from genuine output observations, without
Window timers or animation synthesizing song progress. Unsupported/unavailable
output presentation leaves recorded progression and natural completion pending;
Stop stays available.

Natural termination says **Recorded replay ended**, including interrupted
prefixes. It does not assert the whole chart completed, relabel/rewrite the
input recording or recapture it. Live Play and its optional recording/download
remain independent. Host fixtures are authored with controlled endpoints and
have not run; neither browser/audio playback nor generated bindings are verified.

## Saved records

After a recorded play and both cleanup joins, **Save last recording** explicitly
stores its canonical bytes, original chart path, score and capture label in this
browser. Saving is optional and never automatic. **Refresh saved records** lists
metadata without loading replay payloads. **Use saved replay** selects that
recording for the existing Play replay action and shows the matching chart path;
it does not import song assets or start audio. Select/prepare the matching chart,
then click Play replay. **Delete selected record** deletes only that saved item.

The same-origin library allows 128 records, at most 64 MiB each and 256 MiB total
encoded data. Count/aggregate checks and metadata/byte insertion share one
transaction; deletion also updates both stores together. Success waits for
transaction commit, and no old records are silently removed to make room.
Quota/open/abort/corruption errors remain visible while the current capture,
download and selected replay are retained. A successful save followed by a
failed list refresh remains reported as a save with a refresh error.

Only one library operation owns the controls. Hiding/leaving the page closes
the storage owner and invalidates late replies. Version changes/timeouts fence
the connection; a later explicit action can open a fresh owner. Browser-managed
storage is best effort and may be removed by the browser/user, so explicit
downloads remain useful. Stored complete/prefix labels are display metadata and
do not authenticate a recording or prove it matches selected assets. Browser
competition against saved records remains separate work. Storage/host fixtures
are authored but unexecuted, and no IndexedDB/browser/audio behavior is verified.
