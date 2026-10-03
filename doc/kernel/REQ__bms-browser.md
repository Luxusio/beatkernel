# Browser host and shared selected-file preparation

The browser host belongs to the existing `beatkernel-bms-runtime` application
crate. The BMS adapter and kernel remain independent of DOM and browser APIs.
React is not used. Native and browser preparation must call the same chart,
audio and image algorithms through an asset source boundary.

## Selected resources

The user selects a folder or a set of files. Folder relative paths are retained;
flat selection can resolve only the resource names it actually supplies. Capture
the selected relative path into an explicit message field before transferring
File objects to the Worker; do not depend on browser extension properties
surviving structured cloning. Read
original bytes, not browser-decoded text, so the common UTF-8/Shift-JIS policy
remains authoritative. Resolve assets relative to the selected chart's parent.
Never use chart resource names as network URLs or native filesystem paths.

Memory keys normalize slash/backslash and dot components while preserving
Unicode spelling and case. Empty, absolute, parent, drive-qualified and NUL
paths are rejected. Duplicate normalized keys and file/directory collisions
are rejected, rather than replaced. Audio lookup shares the native literal-first
finite extension-variant policy; images use exact lookup. Native canonical
containment and symlink behavior stay in the filesystem adapter.

The default selected tree permits 32,768 files, 64 MiB per file, 256 MiB total
encoded bytes and 4,096 UTF-8 bytes per path. These are configurable source
limits. The supplied host preflights metadata before sequential file reads;
Rust independently checks admission. Chart parsing, PCM and decoded-image
budgets additionally apply. Replay identity validation precedes asset reads.
Mixed source sample rates remain supported by the existing SampleBank/Mixer
policy; preparation preserves each source rate and adds no pre-resampling pass.
The selected mix format does not imply an actual browser output device.
Image aliases share existing raw/cropped
resources and preserve existing unavailable-image behavior.

## Ownership and lifecycle

Main-thread DOM owns selection, controls, text status and canvas layout. A
dedicated module Worker owns the selected tree, common preparation, prepared
chart/PCM/images, and the wgpu renderer on a transferred OffscreenCanvas. This
avoids running preparation or GPU submission on the main UI thread. It does
not establish a browser audio or gameplay thread.

Imports use generations; late work must not replace a newer selection. One
import pump retains at most one active candidate/read plus the latest queued
metadata request. A superseded unabortable file read must settle before the
candidate is released and the latest request starts. Read files sequentially.
GPU initialization and
mutating WASM calls are serialized. Chart replacement must either publish the
prepared chart and its metadata together or retain the old selection clearly.
A completed file import is staged until the main thread accepts its current
catalog generation. An ignored proposal must not replace the accepted library;
a failed later import retains the same accepted library on both sides.
Render only after a relevant change or bounded surface retry, with at most one
pending presentation callback. Size checks precede GPU configuration; zero
extent suspends drawing. Retain the canvas for recoverable surface recreation.
On page lifecycle teardown, terminate the Worker, cancel pending redraw and
disconnect layout observers; restoration creates a fresh canvas and owner.

Resource names and chart metadata enter DOM through text nodes, never HTML.
Unsupported Worker, OffscreenCanvas, secure context or WebGPU configurations
produce an explicit error. The pinned renderer supplies WebGPU; this host does
not promise a WebGL fallback.

## Preview and remaining player work

The first host displays the actual compiled chart and prepared static BGA at
an explicit original-song time. Nanoseconds cross JavaScript as BigInt or
validated decimal text, preserving signed 64-bit values. Animation callback
timestamps are presentation scheduling only, never an audio clock. The shared
visible-note query, retained GPU note cache and BGA opacity rules are reused.

The host now contains a playable start/stop source path through the common
Runtime and separate AudioWorklet Mixer. The bounded native presentation
observer is reused for continuous input-transport correction. Optional shared
capture/export, local replay viewing and explicit saved-record catalog are
source-integrated. Explicit live WebTransport Play is source-integrated;
the compatible HTTP/3 service and ranked browser competition remain follow-on work. Source integration does not complete or validate the requested player.

## Known ceiling

Selected-file limits bound retained encoded data, not the entire process.
Binding copies, decoder scratch, decoded PCM/images, GPU allocation and overlap
between the current and staged replacement library consume additional memory.
Arbitrary binding callers can allocate before Rust admission; metadata preflight
belongs to the supplied host. Files not selected by the user are unavailable.

Source compilation does not prove generated bindings, WASM linking, Worker
ordering, browser file access, canvas output, playback or physical latency.
Fixtures may be authored and compiled; execution, browser/native acceptance and
formal review remain deferred under the user's verification sequencing.
Deferred Node fixtures cover the actual host metadata/time helpers and Worker
generation/cancellation path with mocked WASM ownership, without requiring a GPU.

## AudioWorklet component and remaining host integration

The optional `browser-audio` build in the same application crate exposes the
actual kernel Mixer for an AudioWorklet. Its generated `audio-pkg/` artifact is
separate from the graphics Worker package. An ordinary Worklet WASM instance
owns its own SampleBank, unique queue producer/consumer, Mixer and fixed output
storage. A pointer from the gameplay Worker's WASM instance is never treated as
shared storage. Prepared PCM is transferred and copied during setup, preserving
sample identity, source rate and explicit channel layout. Rendering performs no
decoding or application allocation. Setup/destruction remain outside `process`.

The Worklet uses the actual AudioContext sample rate. Its one-shot start target
is an absolute context frame, acknowledged separately from a posted request.
Prestart silence and a partial first block are exact on that grid. Mixer frame
zero starts at the selected absolute frame; subsequent context blocks must be
contiguous. Invalid/duplicate/late start, discontinuity, overflow, mismatched
channels or oversized blocks produce explicit failure. Actual callback lengths
are used; 128 frames is not a permanent assumption. A fixed interleaved WASM
view is copied into the browser's supplied planar outputs. Unexpected memory
growth after activation is a terminal error rather than reuse of a detached view.

Numeric command batches preserve IDs, timestamps and order, with bounded count,
session generation, sequence and admitted-prefix acknowledgement. Local Runtime
admission, Worklet queue admission and Mixer execution are distinct evidence.
An admission failure fences the session; committed game operations are never
retried. BGM's existing rolling feeder stays with the gameplay owner. Polling
the Worklet returns actual render/queue counters outside the per-block callback.
Those reports do not establish output presentation or acoustic latency.

Pinned generated bindings may require UTF-8 TextEncoder/TextDecoder in a Worklet.
A compatibility bootstrap runs before those bindings and installs only missing
implementations of the non-streaming UTF-8 operations they use. It preserves
typed buffer offsets, scalar replacement, BOM/fatal behavior and encodeInto
counts; unsupported encodings/streaming fail explicitly. It is not a general
encoding polyfill. The steady numeric render path does not encode strings.

The processor starts silent, admits resources, finishes allocation, arms once
and acknowledges stop before its owner is discarded. Failure publishes one
terminal diagnostic and silences output; it never silently restarts. A context
owns one active processor for this generated module. JavaScript/GC/MessagePort
and browser device buffering have no hard realtime guarantee. Native WASAPI or
ASIO controls are not browser capabilities.

The browser audio host owns one fresh AudioContext and Worklet node per session.
Opening is called from a user gesture and requests context resume before any
asynchronous module initialization. It uses the actual context rate and a
precompiled audio WASM module. Setup cancellation uses an optional AbortSignal;
a pre-aborted request creates no context. Resume, module loading and readiness
share a finite setup deadline. Read-only metadata exposes the actual sample rate
for later preparation without exposing mutable context ownership.
The read-only current frame estimate floors context time multiplied by its
sample rate, validates safe integer range and returns BigInt. It is a control
estimate for choosing a future arm target; Worklet callback chronology remains
authoritative, and the estimate is not output/acoustic evidence.
Readiness, resource admission, one-shot arming,
command-prefix acknowledgements, render reports and shutdown have distinct
states; one ordinary control operation may be in flight, with no unbounded host
queue. PCM transfer consumes a standalone full backing buffer during setup.
Opening requires a running context before returning a usable owner. If a usable
context becomes suspended, interrupted or closed, the host fences and cleans
that owner rather than silently advancing a session against paused audio.
Invalid local requests reject before posting. Partial command admission or
transport/processor failure fences the owner and never retries committed input.
Explicit idempotent stop cancels pending work, attempts bounded acknowledged
Worklet cleanup and disconnects/closes the context even when acknowledgement
fails. Neither ACKs nor context time establish acoustic presentation.
A timed-out context-close promise is an explicit cleanup failure; bounded host
waiting does not prove that the browser released its internal audio resources.

The player UI calls these components through prepared-sample transfer, a
nonblocking shared SoloRuntime and bounded input/audio message ownership.
Actual getOutputTimestamp pairs feed the existing native presentation
discipline. Optional common capture exports canonical replay bytes. Local
replay playback reuses the output host, with explicit saved-record selection.
Live browser networking and the optional HTTP/3 relay are source-integrated;
ranked competition remains unfinished. Browser input timestamps use
the originating Window performance domain; Worker and Window origins are not
implicitly equal. Physical keyboards cannot be distinguished by DOM key events.
The full player Goal remains open, with generated bindings, processor execution,
audio/device behavior and formal acceptance still deferred.

## Nonblocking gameplay integration

The browser gameplay owner executes the existing SoloRuntime, JudgeEngine,
ScoreSummary and rolling BgmFeeder. It has no native polling loop, OS clock or
audio callback ownership. Software profiling is disabled until an explicit
browser processing clock is configured. Actual prepared sample identity, rate
and channel layout survive transfer into the separate Worklet memory.

Input and deadline messages retain the originating Window performance domain.
One FIFO boundary carries bounded event batches and explicit advancement
watermarks; graphics Worker timestamps never replace physical event timestamps.
Actual core reports drive song time, judgments and score. Unsupported or late
input is rejected explicitly, without silently moving its time. The browser
currently exposes one logical keyboard, with explicit code/lane bindings.

The gameplay owner retains one bounded outgoing command batch awaiting Worklet
acknowledgement. Core input/advance operations may continue against the bounded
local queue; they never wait synchronously for MessagePort/audio completion.
Exact full-prefix acknowledgement releases the outgoing batch. Partial
admission, correlation failure or local queue failure fences the session and
retains committed evidence; neither inputs nor commands are retried. BGM rolling
credit uses actual completed Mixer cursor reports, not UI elapsed time.

Play setup requires a user gesture. Stop/focus/page lifecycle cancels preparation
and closes audio/game ownership; library/preview mutation is fenced while a live
owner exists. Full output completion requires actual judge, mixer and output
presentation evidence and is not inferred from the last note or a timer.


The supplied Play path re-prepares the selected chart at the actual AudioContext
sample rate, transfers each original PCM asset once and acknowledges initial
BGM admission before choosing a future absolute start frame. A pristine-only
activation reanchors the same shared runtime after setup; this is not a seek.
Window performance and AudioContext time are bracketed for a nominal software
start projection. This projection does not compensate acoustic latency or clock
drift. The AudioHost outputTimestamp accessor supplies genuine browser pairs
to bounded continuous rate correction after accepted gameplay watermarks.
An initial projection remains an estimate and past judgments are not revised.

The DOM admits at most 1,024 queued key events and sends at most 256 per step.
Only one step, one render-report request and one outgoing audio batch await
correlated acknowledgement at each boundary. No unbounded MessagePort backlog
is used to hide a delayed Worker. Deadline or capacity failure stops the owner.

Stop preserves the actual available score before game disposal and waits for
both audio cleanup and a correlated Worker release before re-enabling controls.
Pending setup cancellation reports null score when no runtime exists. Page
teardown or a bounded missing-stop-receipt deadline terminates Worker ownership;
a missing receipt or failed audio cleanup requires reloading before further play. The saved accepted
preview metadata and position are restored after ordinary stop. The automatic
completion source path below also uses this cleanup handshake. Source, fixtures
and cargo check evidence are distinct from behavioral acceptance.

Audio opening failures preserve the original setup error and attach cleanupError
when bounded stop/close also fails. Cancellation waits for the opening promise
and propagates this cleanup evidence before releasing the UI owner. Failed
opening cleanup requires reload even when no usable AudioHost was returned.


## Browser automatic song completion

Natural completion must use the existing shared SongCompletion evidence: every
original chart object resolved after its inclusive deadline, complete BGM
admission/render credit, no queued or awaiting-ACK producer work, then a later
idle Mixer block and an actual reported output position past that block. Neither
a chart duration, last-note timer nor admitted command count proves completion.
Any new producer work resets the pending drain barrier before re-observation.

The supplied host obtains actual AudioContext output timestamps and converts
context position to the armed Mixer grid conservatively with integer arithmetic.
Only fresh, nonregressing reports are forwarded; unavailable, future, stale,
zero or prestart reports remain unavailable. Elapsed UI time never advances a
reported output cursor. The browser's estimate does not prove acoustic latency.

Completion retains the gameplay owner until the Window's captured input prefix
and outstanding steps/audio batch join, then uses the same bounded stop/disposal
handshake. A manual stop, cancellation or failure does not become a natural
finish. A browser without usable output timestamp evidence keeps manual Stop
available and cannot claim natural completion. Replay playback and explicit
record storage use separate owners below. Live multiplayer uses the shared
session, with the optional HTTP/3 relay source-implemented and ranked competition
unfinished; this acceptance contract is not evidence that source was executed.

A gameplay disposal error during completion or manual stop is retained as a
cleanup failure. The Window terminates that Worker and requires reload before
another play; an earlier natural-finish candidate does not hide the error.

The numeric Worklet-report decoder belongs to the existing portable application
audio boundary. WASM bindings delegate to its single implementation; direct
decoder fixtures can run as ordinary host Rust tests without generated bindings
or a browser. This changes no PCM callback or serialized byte format.

## Bounded input/output clock discipline

The browser gameplay owner shall reuse the existing portable native
PresentationDiscipline with preallocated bounded observations. Actual paired
output position and Window performanceTime estimate provide its only evidence;
accuracy remains Unknown. Missing, stale or duplicate evidence shall not invent
progress or refresh the observation age. A nominal transport remains available
when evidence is absent. Coarse host time without host progress defers admission.

Correction shall apply only after an accepted complete input prefix and host
watermark, continuously at that same host instant. Captured event timestamps,
committed judgments, past transport history and scheduled original audio
commands shall remain unchanged. Explicit rate/phase/domain/chronology/overflow
failures shall fence the owner and retain committed score. Freshness is one
second; all other initial policy bounds use the existing native default. This
policy is an estimate-based drift correction, not an acoustic timing guarantee.
Native shared callers remain opt-in. Completion still consumes actual output
positions independently of transport correction.

## Optional shared recording and replay export

Recording shall be an explicit optional choice, disabled by default. The shared
StepGameplay owner shall reuse LiveReplayCapture and the canonical replay codec,
retaining actual RuntimeReport inputs, original provenance and accepted song
time, including continuous clock correction. The prepared chart's actual branch
seed shall be carried into the existing versioned replay metadata. Native/shared
callers remain opt-in and audio callbacks acquire no recording work.

The browser policy shall cap encoded data at 64 MiB and accepted operations at
1,000,000. The byte cap is not a complete process-memory cap or unlimited-song
recording promise. Capture failure shall fence the run, retain committed score
and the prior replay prefix, and not retry accepted inputs. Disabled capture
shall not constrain song duration by these recording limits. Canonical export
shall be available only after stopping/fencing the owner and taken at most once.

The Worker shall always attempt game release, separately reporting serialization
and resource cleanup errors. It shall transfer standalone bounded encoded bytes
before freeing the WASM game. Cancellation/failure yields a prefix; a complete
label requires genuine natural-completion evidence and both game/audio cleanup
joins. The Window shall retain at most one result and offer an explicit download
only after joins. Export URLs shall be revoked on replacement/page hiding. No
auto download, saving or playback shall be triggered by export. Replay and
explicit saved-record library actions are defined separately below.

## Nonblocking replay component

Audible browser replay shall reuse canonical decoding, recorded seed/section
selection, ReplayVisual, the existing replay audio planner, rolling feeder and
ReplayCompletion. A nonblocking application owner shall retain at most one
bounded immutable command batch until actual remote ACK. Live and replay shall
share output-evidence and ACK validation, not duplicate clock/judge algorithms.

Actual output presentation alone shall advance recorded visual operations and
score. Rendering credit shall come from genuine checked Mixer reports. A log
prefix shall remain a prefix: no synthetic final advance, new judgment, BGM after
its recorded end or chart-complete claim. Original equal-time ordinal order,
negative preroll and recorded section/branch provenance shall remain intact.
Completion shall require recorded-operation exhaustion and subsequent actual
PCM drain/presentation, independently of command admission. A failed owner shall
retain readable score/state and refuse further consuming operations.

Browser replay preparation shall own the decoded bounded recording together
with its actual selected assets. Live BrowserGame shall reject that replay
resource; replay shall consume it once through its dedicated owner. The common
canvas/renderer shall draw replay score, note progress and actual pressed lanes
with the same primitives as live play. Building these components does not imply
that Window/Worker replay launch, browser execution or physical audio is verified.

Live and replay pressed state shall use the common canonical eighteen-lane BMS
mask, including sparse charts and player-two lanes. A displayed lane's position
in the chart shall not change its control bit or highlight a different lane.

## Audible replay host

The browser shall offer explicit selection of one local recording and a separate
Play replay action using the selected matching chart/assets. Live Play shall
remain independently available. Window and Worker shall validate nonempty
recording metadata at or below 64 MiB before acquisition; the Worker shall read
it once, reject changed size/invalid layout and invalidate cancelled preparation
before constructing a WASM owner. Playback shall not upload or automatically
save its recording. Explicit library actions are defined separately below.
The canonical file supplies branch seed and section, not live controls.

Replay shall reuse the same AudioHost, command/sample transport, immutable
armed start, genuine output presentation and joined cleanup as live play.
Audio resume shall remain within the initiating user gesture. Replay shall
reject live input steps and accept no synthetic host-clock advancement or live
capture. Readable recorded score shall update from output observations.
Natural termination shall say the recorded replay ended, including prefixes,
and shall never assert full-chart completion. Stop, Escape, focus/page loss and
failures shall release the actual replay/audio owners. Source integration and
authored host fixtures do not imply browser/audio execution was verified.

## Explicit local recording library

The browser shall offer explicit Save last recording, Refresh saved records,
Use saved replay and Delete selected record actions. No automatic save, upload,
pruning or deletion shall occur. Saved bytes shall be the actual canonical
capture extracted after both cleanup joins. Their stored chart path and readable
score are display metadata; a complete label shall never authenticate a file or
prove the selected chart/assets match. Loading shall select a replay for the
existing user-gesture Play replay path without automatically starting audio.

The same-origin IndexedDB boundary shall separate metadata from encoded bytes.
Listing shall read bounded metadata only. Initial limits are 128 records, 64 MiB
per recording and 256 MiB total encoded bytes. Save shall validate metadata and
bytes before admission, then check aggregate limits and insert both stores in
one transaction. Delete shall remove both stores atomically. Write success shall
require the transaction's complete event; request success alone is insufficient.
Quota, abort, corrupted data, blocked/failed opens and unavailable storage shall
remain explicit errors without losing the current replay or live/download path.
Browser-managed storage is best effort and may be removed by the browser/user.

One library operation shall own the UI at a time. Closing/hiding the page shall
invalidate late results, close its connection and abort pending transactions;
late opens shall release their connection. Version changes and timed-out
operations shall fence the storage owner. A new explicit action may open a
fresh owner. Stale results shall never replace current selection or page state.

Signed replay/preroll readout shall format one sign and the absolute exact
nanosecond magnitude, including negative subsecond time and i64 minimum.
Preview time admission shall remain nonnegative. Browser storage, binding,
audio and authored fixture execution remain deferred until scheduled.


## Explicit live multiplayer and output start

Solo Play shall remain the default automatic audio path. An explicit live-only
multiplayer option shall accept a bounded HTTPS WebTransport endpoint and host
proposal/join role. Replay shall stay local. No selected assets or raw keyboard
events shall be uploaded. A compatible HTTP/3 server and trusted certificate
are required externally; the native raw QUIC listener is not that endpoint.

The Worker shall derive exact setup identity from its actual pristine BrowserGame
and use the existing shared Rust session through BrowserMultiplayerOwner. It
shall open the connection and request readiness only after actual sample import,
audio finalization and initial command acknowledgements. It shall translate the
committed elapsed target using explicit Worker/Window performance time origins;
wall-clock Date.now, receipt timestamps and guessed origins are not substitutes.

A fresh bracketed audio clock shall project the committed Window target onto
one immutable output frame, rounded upward. Game activation shall receive the
same host origin projected from that frame. At least 100 ms of preparation lead
and at most 100 ms combined peer/bracket uncertainty shall be required. Missed
activation shall fail setup instead of choosing a different start. Browser clock
coarsening and physical output latency remain measurement limitations; source
integration does not guarantee acoustic synchronization.

Actual local cumulative song/score/max-combo getters shall produce bounded
progress updates, with at most one pending submission and no network operations
in input/audio callback threads. Remote self-reported scores shall appear
separately and shall neither replace local judgment nor claim ranked authority.
Pre-start network failures shall fail preparation. During active play, network
loss shall remain visible while local gameplay continues.

Stop shall invalidate the gameplay owner before disposing it, retain actual
readable score and replay prefix, and attempt a final progress write and genuine
peer application ACK under one finite two-second cleanup deadline. Audio stop
shall not abort that independent network drain. Missing ACK or network loss shall
be labelled explicitly; a local write is not a peer receipt. All stale callbacks
and timers shall remain fenced from later sessions. Browser/network/audio QA and
fixture execution remain deferred; the persistent player task stays open.

## Saved-opponent component and gameplay binding

Browser saved-record competition reuses the actual common Competition/replay
judge with the pristine local chart/rules/profile/branch identity. Admission is
bounded to eight recordings and a 64 MiB aggregate encoded budget, charged only
after successful validation; display labels are bounded plain metadata. Prefix
files show only their recorded operation results and recorded_until frontier.
Advancing beyond that frontier never invents misses or implies completion.
Local gameplay score remains independent and is aggregated once. Comparison
errors must stay separate from local play/capture completion. A fresh owner and
explicit reset govern restart; post-activation admission is refused. Source
components and WASM bindings do not establish host selection availability or
actual browser execution; Window/Worker integration follows separately.
