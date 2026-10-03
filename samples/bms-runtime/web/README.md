# Browser BMS player

The optional `browser` feature belongs to the existing `beatkernel-bms-runtime`
application crate. This host imports user-selected files, prepares chart/audio/
image data through the common Rust algorithms, and displays compiled notes and
static BGA at an explicit song position. The Play source path also transfers
prepared PCM to an AudioWorklet and feeds originating Window keyboard timestamps
into the same SoloRuntime used by native gameplay. The natural-completion source
path joins terminal judging, Mixer drain and reported output timestamps.
Optional capture/download and local canonical replay playback are source-wired
to this host. Browser multiplayer execution remains unverified. The shared Rust
`multiplayer_protocol` module provides bounded BKMP v6 framing and the same
progress, readiness, clock-probe and final-acknowledgement state used by native
QUIC. Its common session owner composes setup matching and software start
agreement, with bounded events and exact complete-write receipt IDs. Native
QUIC already delegates those transitions to the common owner. Callable
`BrowserMultiplayer` WASM bindings and `multiplayer-transport.mjs` provide the
session and WebTransport stream boundaries. The Window/Worker live Play source path now creates those owners after
local audio preparation, maps the committed start onto the actual output grid
and publishes genuine score summaries. The optional native HTTP/3 relay is source-implemented in the same app; this
path has not been executed.

The DOM owns file selection, controls and layout. A dedicated module Worker owns
the imported bytes, preparation and wgpu rendering on a transferred
OffscreenCanvas. The host uses plain JavaScript without React or external
scripts. Existing retained note instances and BGA texture caching are reused.

## Multiplayer component boundary

`BrowserMultiplayer` owns the shared Rust session. Its constructor takes exact
canonical setup bytes, a host/join role and actual preroll. `with_policy` exposes
the software start bounds. `WebTransportChannel.open` in
`multiplayer-transport.mjs` owns the browser stream. Explicit live multiplayer
Play calls these components after samples and initial audio commands are acknowledged.

`BrowserGame.competition_identity()` derives those bytes from the actual pristine
gameplay judge and resolved chart seed without configuring capture. The
`BrowserMultiplayerOwner` in `multiplayer-owner.mjs` drives a supplied session
and channel with bounded duplex loops. `open` requires a `now` function returning
BigInt nanoseconds on the caller's host clock; the owner's `origin` maps elapsed
software targets back to that same coordinate system. `request_ready` follows
actual preparation. `submit(progress, finalPrefix)` resolves a complete local
write, while `wait_final_ack` observes the separate final application ACK.
One submission and one final waiter are permitted. The Worker instantiates
this owner for explicit multiplayer live Play; solo and replay remain local.

An integrating owner requests readiness after preparation, reads at most
`needed_bytes()` using `readPrefix`, then passes that bounded prefix to
`receive_bytes` with explicit elapsed nanoseconds. Drain `poll_event` after each
operation. `next_write` returns kind 0 for waiting, 1 for a frame or 2 for an
application slot. Frame bytes may be taken once; only after `channel.write`
resolves may `written(frame_id, now_ns)` credit their complete local write.
An application slot permits `send_progress` from the actual game counters.
Free each write object after consuming it, and close/free the session during
cleanup. Times, frame IDs and counters use BigInt. The returned start event is a software
schedule; the caller still owns audio startup and deadline admission.

The channel allows one read and one write together, with finite deadlines.
Read prefixes and writes are limited to 65,547 bytes; received chunks and their
retained backing buffers to 1 MiB. Empty chunks are skipped at most 16 times.
Cancellation and remote closure fence late completions. No peer application ACK
is inferred from a local write. A compatible HTTPS HTTP/3 service remains
required; actual generated bindings, browser/network/audio and host fixtures
have not been executed.

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
Optional capture/download and replay use the same host. Explicit live multiplayer
uses its committed output start; the optional HTTP/3 relay is source-implemented.

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

The page shows the actual lane bindings. By default, the first keyboard side uses left
Shift for scratch and Z/S/X/D/C/F/V for seven-key lanes; Space covers the
additional lane. The second uses right Shift and N/J/M/K/Comma/L/Period, with
Slash for its additional lane. DOM keyboard events provide one logical device.
Stop, Escape, focus loss and page hiding cancel the session. Controls remain
locked until audio and Worker ownership are released; ordinary stop restores
the accepted preview and reports the available actual score.

Expand **Keyboard bindings** to change the retained lane selectors before live
play, or choose **Reset keyboard defaults** while idle. The catalog offers 80
physical key codes; Escape remains Stop. Choose **Unbound** only for lanes absent
from the prepared chart. Duplicate assigned keys and missing required mappings
are refused. One immutable selection supplies both Worker key pairs and Window
input handling and captions. Import, preparation, playback and record operations
lock the editor; failure or Stop retains the draft. Replay ignores the live draft.
Settings last for this page only. Browser and system shortcuts can prevent key
delivery. The IDs belong to this browser host, not native OS scan codes.
These source paths and authored fixtures have not been executed in a browser.

Output preferences apply to live and replay playback. Choose **Interactive**
(default), **Balanced**, **Playback**, or **Custom** latency. Custom accepts
0–60000 milliseconds with up to six decimal places and no signs, spaces or
exponents; category modes ignore the
inactive custom field. Leave the requested output rate blank for automatic
selection, or enter a positive integer fitting u32. An unsupported browser
request fails without retrying default settings. Each launch captures one
selection before audio setup; busy operations lock these page-local drafts.
The actual opened context rate still controls chart/audio preparation. Replay
keeps its recorded judge settings and section. Latency hints are browser
preferences, not guaranteed callback sizes or measured acoustic latency.

Advanced output capacities let both live and replay choose the command queue
(1–65536), simultaneous voices (1–4096), pending commands (1–4096), maximum
render frames (1–4096) and command processing budget per render (1–65536).
All default to 4096. Enter unsigned decimal integers, without signs, spaces,
exponents or fractions. Each launch freezes one selection for AudioHost; Worker
command batches are independently bounded to min(256, selected queue capacity).
Controls lock while busy, and failure or Stop retains the page draft.

These are application allocation and processing limits. They do not select
hardware buffer sizes. A browser callback larger than the chosen frame capacity
fails, and queue, voice or pending limits must fit the chart's workload. Initial
BGM must fit the queue before the armed start, when commands are not drained.
Smaller batches preserve acknowledgement sequences and rejected prefixes;
they do not retry commands or guarantee every workload fits a small queue.
Frame and pending ceilings retain the existing Rust browser report contract.

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
the replay path. Live WebTransport Play and the optional HTTP/3 relay are source-integrated;
ranked browser competition remains unfinished.

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
Native replay tools decode ordinary legacy captures using matching chart
setup; finite v4 captures still require native consumer integration. The setup
fingerprint is noncryptographic and excludes device/audio-file identity. A renamed file does not prove complete playback.

Only one result is retained. Downloads are explicit; no file is automatically
written or uploaded. Blob URLs are created only on a download click, revoked on
replacement/page hiding and after at most 60 seconds. Stopped capture extraction
is a single encoding attempt; serialization failure remains explicit and does
not hide game cleanup failure. Local replay import/playback is source-integrated;
Explicit saved-record storage/browsing feeds that replay path; browser
record competition and live network summaries are source-integrated.
Source checks and authored fixtures
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
competition against saved records is source-integrated. Storage/host fixtures
are authored but unexecuted, and no IndexedDB/browser/audio behavior is verified.


## Multiplayer live Play

Leave **Multiplayer live play** unchecked for the usual solo path. To use the
source integration, enable it and supply a compatible HTTPS WebTransport URL.
Select **Propose start** for one participant and **Join start** for the other.
Prepare the same chart/seed and matching gameplay/output setup on both sides.
The URL needs a trusted certificate and an HTTP/3 service that pairs two BKMP v6
streams. The optional `serve-multiplayer` mode supplies that HTTP/3 adapter;
the native raw QUIC listener remains a separate transport. Actual
interoperability is unverified. Selecting multiplayer never
uploads chart assets or raw keyboard events. Replay remains local even while
this option is selected.

Audio resumes within the original Play gesture. Samples and initial commands
finish first; only then does the Worker create the actual multiplayer session
from BrowserGame's canonical setup identity and request readiness. The agreed
elapsed target maps through explicit Worker/Window performance time origins.
A fresh bracketed audio clock rounds upward to one output frame and its host
projection is used for both AudioHost.arm and BrowserGame.activate. Insufficient
lead, excessive uncertainty, an already-rendered frame or missed activation
fails preparation instead of silently choosing another start. Software clocks
and device output latency still limit physical synchronization.

A separate readout labels the peer's self-reported counters. They do not replace
local judgment or establish ranked results. During active play, connection loss
leaves local gameplay running. Stop disposes gameplay first, then attempts the
actual final score prefix and separate peer application ACK within a two-second
network drain. Missing ACK is reported separately from local-write success;
network errors do not relabel a genuine locally completed recording as a prefix.
The page's HTTPS connection policy allows the explicitly selected server.
Source fixtures and compile-only checks are preparation for later validation,
not evidence of browser playback or multiplayer execution.


The common application crate also provides a bounded room/participant ownership
foundation used by the optional HTTP/3 service. It returns precise closure leases,
expires only waiters and fences stale disconnects from newer same-key rooms.
The registry performs no socket/TLS/HTTP/3 work; the optional adapter owns
actual sessions and applies those closure leases. Current BKMP remains bilateral; local-player extensibility does not
by itself provide a multi-party network protocol. See the
[room ownership component](../../../doc/changes/CHANGE__multiplayer-room-ownership.md).


## Optional HTTP/3 relay

From the repository root, build the same application with `webtransport` and
run its server mode when execution is scheduled:

```sh
cargo run -p beatkernel-bms-runtime --no-default-features --features webtransport -- serve-multiplayer --bind 127.0.0.1:9001 --cert server.pem --key server-key.pem --origin http://localhost:8080
```

Both players use the same `https://localhost:9001/rooms/example` URL. Supply a
certificate trusted by the browser and the exact page origin, including port.
Repeated `--origin` flags admit additional explicit origins. HTTPS origins and
HTTP loopback page origins are accepted; the endpoint itself always uses HTTPS.
There is no self-signed certificate generation or verifier bypass. Missing
Origin is rejected by default, with `--allow-missing-origin` an explicit native
client opt-in. Native gameplay may select raw QUIC or the optional WebTransport relay client.

The relay accepts one bidirectional stream per participant, validates bounded
BKMP v6 frames and forwards them between the room's two participants. Shared
session code retains compatibility, readiness, software start and peer ACK
semantics. Admission, setup, waiter and I/O deadlines are bounded. On EOF the
opposite direction has at most two seconds to drain; abnormal failure closes
both participants. Ctrl+C closes the sessions and joins outstanding tasks.
Server limits and configuration are listed by `serve-multiplayer --help`.
These commands and browser/TLS/network interoperability remain unexecuted.
See the [adapter contract](../../../doc/changes/CHANGE__webtransport-relay.md).


A native participant can join the same room using the optional `webtransport`
build and `play`/`player` flags `--mp-webtransport HTTPS_ROOM_URL --mp-role
host|join --mp-origin ORIGIN --mp-ca PATH`. Both sides need matching canonical
setup and opposite start roles. The native client supplies the exact configured
Origin and verifies the HTTPS URL server identity against its explicit CA.
These source paths share the existing BKMP session; actual native/browser
interoperability remains unexecuted. See the
[native adapter contract](../../../doc/changes/CHANGE__native-webtransport.md).


## Saved-opponent component boundary

`BrowserGame.add_saved_opponent(bytes, own, label)` admits a compatible saved
record before activation. `saved_opponents()` returns the bounded actual
comparison snapshots. The browser gameplay bindings expose a component for compatible saved
Own/Other records before activation. Its counters come from the common replay
judge and actual recorded operations, with an explicit recorded_until frontier.
Local scores remain separate; a truncated record never gains invented misses
when live play passes its final operation. Admission and comparison failures
do not become ranked proof or change capture completion. Window/Worker selection
controls now compose these bindings as described below. Browser execution is
unverified. See the
[component contract](../../../doc/changes/CHANGE__browser-saved-opponents.md).


## Live saved-record competition

Select an Own/Other display kind, then add the current imported replay or a
record from the local Saved records list. An optional plain label overrides the
filename display. The selection holds at most eight Files totalling 64 MiB;
Remove and Clear release selected slots and bytes. Selection changes are disabled
while loading a saved record or playing. Repeated selection of the same source
is refused. This metadata does not authenticate whose record it is.

Prepare the matching chart/seed and choose Play. The Worker reads selected Files,
validates their actual bytes and canonical setup, then admits them to the real
BrowserGame before activation. Incompatible selected recordings fail preparation
explicitly. Files stay selected for retry and are not detached by Play. The
original audio resume gesture is preserved. Play replay ignores comparisons;
normal live play can use them together with multiplayer. Recording bytes and
chart assets are never uploaded to the relay.

A separate comparison readout shows actual recorded-prefix counters at most four
times per second. It identifies the last recorded operation, including an empty
prefix. Passing that frontier never invents misses or establishes completion.
Local score and peer-reported network data remain separate. Comparison failure
stops comparison updates while local play, audio and capture continue. Old or
closing play owners cannot replace another session's readout.

JavaScript boundary fixtures are authored but unparsed/unexecuted. Generated
bindings, actual browser/audio behavior, interoperability and formal acceptance
remain deferred. See the
[host contract](../../../doc/changes/CHANGE__browser-saved-opponents-play.md).


## Live judge timing

Set Early window, Late window and Input offset before live Play. Values are
milliseconds, with up to six decimal places (one nanosecond precision). Defaults
are 50 ms early, 50 ms late and zero offset. Windows must be nonnegative; offset
may be signed. Decimal text is converted through BigInt directly, including the
full signed 64-bit nanosecond boundary, without floating-point rounding. Spaces,
exponents, excess precision and out-of-range values are rejected.

A live launch retains one immutable timing snapshot before asynchronous audio
preparation. Busy controls prevent changes during preparation, recording-store
operations and playback; values remain editable for a retry after cleanup. The
Worker validates the snapshot independently and passes it to the existing Rust
BrowserGame constructor. Actual constructor limits remain authoritative. Replay
uses its recorded judge and ignores these live fields. Saved opponents and live
multiplayer use the genuine resulting profile for compatibility.

Six focused fixture groups were authored for exact boundaries, Worker routing
and Window lifecycle. They have not been parsed or executed; browser, audio,
formal review and QA acceptance remain deferred.


## Live section start

Set Live start (seconds) before Play live to begin at an original-song position.
Zero plays from the beginning. Enter nonnegative decimal seconds with at most
nine fractional digits; the existing exact parser reserves timestamp headroom
for lookahead. The draft remains available for retry and is locked while the
page's preparation/play/record-store owner is busy. Replay ignores this field
and uses its recorded section.

Each live start prepares fresh original chart/audio assets. Earlier note heads
and whole crossing holds are excluded by the same native section policy.
Overlapping background music selects frames from original PCM once; judging
and graphics keep original song coordinates. Selected saved recordings and
multiplayer peers must match the genuine resulting section identity.

The output host's bounded sample capacity is 5392: 1296 original samples plus
at most 4096 crossing music suffixes. The 64 MiB asset and 256 MiB aggregate PCM
limits remain. This does not prove acoustic synchronization or gapless restart.
Browser execution, generated bindings, test execution, formal review and QA
remain deferred.

## Finite practice component status

The output component accepts an optional exclusive playback frame endpoint in
AudioHost.finish(endFrame), independently validated as a u64 BigInt and forwarded
through the actual Worklet to BrowserAudio and the common Mixer. Omitting it
keeps ordinary unlimited finish. Mixer output ends exactly at the configured
frame and fills the rest of the callback with silence; context callbacks still
advance. The relative endpoint starts after Worklet prestart silence, so its
absolute context position adds the armed start once. Arm refuses overflow.

The supplied page forwards recorded finite replay endpoints into finish(endFrame)
and uses ordinary finish for unlimited playback. Live start/end controls use
the common finite gameplay owner and forward its actual end/frame metadata
through the same validation and output setup. Both stepped owners have
section-aware completion paths. Genuine Mixer and host/Worklet fixtures are authored;
execution, generated bindings and full finite-game/replay acceptance remain pending.


The section-aware report decoder additionally compares finite telemetry against
an explicit configured endpoint. It preserves the actual physical context cursor
and admits the frozen Mixer state at and after the fence. The ordinary decoder
still requires unlimited output; configured finite live and replay owners use
the section-aware decoder.


Finite stepped replay preparation and the Rust browser replay binding now retain
the recorded endpoint and validate actual finite output reports. Completion
requires the actual fence, presentation crossing and finished recorded/command
work. Window/Worker read and independently validate recorded end/frame metadata
against the prepared original start and actual output rate, then pass the frozen
endpoint into AudioHost setup. Unlimited metadata remains omitted and finish()
retains its argument-free call. Invalid finite metadata fails setup without an
unlimited retry. Live end drafts are optional original-song decimal seconds;
a blank end keeps ordinary playback. Finite completion captures retain their
section endpoint. Protocol fixtures are authored but not executed; browser
output is not yet verified.


Gameplay canvas and local/replay score HUDs belong to the graphics Worker. The
Window does not repeat score/status or song-position DOM updates on step/render
responses; preview/menu controls and final summaries remain event-driven. Input
acquisition is intended to support keyboard, touch/pointer and HID through common
physical-event types, preserving acquisition time and source identity. The live keyboard route is connected; the touch bridge below is being added.
WebHID adapters, opponent HUD migration and remaining host-bridge work are pending. Main-thread performance is unmeasured.


The physical browser API is authored beside the existing keyboard API.
Its explicit binding setup retains Any/exact device selectors and HID, native
or vendor controls. Canonical BKPI input blobs preserve core physical variants
and acquisition provenance; configured byte budgets bound decoding. Unsupported
clock domains are refused rather than retimestamped. This API alone does not
provide WebHID acquisition, raw-report decoding or a playable touch interaction.
Those adapters and application controls remain separate integration work.

`BrowserGame.new_physical` accepts seven-word physical binding rows, optional
original-song end and encoded/payload byte budgets. `input_blob(bytes, audioNs)`
accepts exactly one canonical BKPI event in the original Window host clock
domain `0x57494e`, preserving its acquisition timestamp and sequence. The live page
now requests this physical route explicitly and checks matching preparation
metadata before PCM transfer. Worker compatibility callers may still omit the
route to use the old keyboard API. Generated-binding/runtime acceptance remains
pending.

Keyboard acquisition remains on Window while packet encoding runs on Worker.
The Worker validates and encodes the entire bounded input batch before its first
runtime call. Physical keyboard controls use Native browser backend `0x574b4559`
with historical adapter key IDs; these IDs are not USB HID usage values. Native
metadata retains the original Window acquisition point. A zero-lane chart may
use empty bindings; charts with lanes still require complete coverage.

## Actual browser touch bridge

The live page exposes a pre-play touch policy, enabled automatically on
touch-capable PointerEvent browsers. It selects `physical-contact` mode and
keeps keyboard input available. The choice locks for the session; recorded
replay uses its recorded rules. Different contact/button-only rule identities
are not silently merged for competition.

Window collects genuine canvas touch pointers, original event timestamps,
native pointer provenance, generated contact identities, coordinates and
pressure. It retains pointer capture through release/cancel and shares the
bounded acquisition queue and sequence with keyboard input. Cached surface
dimensions avoid per-event layout queries. No lane selection or canonical
packet serialization happens in the input callback.

Worker serializes canonical BKPI Touch packets, validates the entire mixed
batch before gameplay adoption, and supplies projected hit coordinates
separately to `input_blob_at`. Regions and dimensions come from the same
Rust layout used by rendering. Moving outside a lane keeps the original
contact owner. Physical coordinates and acquisition time remain in captures.

Missing pointer-capture or contact-runtime capabilities fail explicitly.
This bridge has source changes and deferred fixtures; generated bindings,
browser/touch-device execution, WebHID and full native/contact record
compatibility remain pending.

## Contact pressed-lane feedback

Live browser feedback and recorded replay presentation use the same bounded
`PressedKeys` ownership component. Genuine admitted Touch Down adds a separate
contact owner; matching Up/Cancel releases it. Move and unknown releases do
nothing. Contact ownership includes source, surface, game control and the full
contact ID, independently of button ownership. A lane remains pressed while
any button or contact owner holds it. Feedback does not select lanes from raw
coordinates or depend on hit judgement.

This source integration includes deferred portable fixtures. Rust compilation
and authored fixtures do not establish actual browser presentation acceptance.
