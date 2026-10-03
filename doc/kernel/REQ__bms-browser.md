# Browser host and shared selected-file preparation

## Performance-first browser thread ownership

The Window main thread must do as little application rendering as possible.
Continuous gameplay rendering, including HUD and score presentation, belongs to
the graphics/game Worker through OffscreenCanvas. Do not assemble scenes, draw
gameplay canvases, update a DOM gameplay HUD each frame, judge inputs, decode
assets or serialize gameplay/network data on the input acquisition thread.

The Window collects supported keyboard, touch/pointer, HID and other physical
input events and forwards a bounded, ordered prefix through the common input
abstraction. Preserve acquisition timestamps, source/device/contact identities
and original clock provenance; Worker arrival time cannot replace event time.
Permission refusal, cancellation, disconnect, focus loss and released contacts
must have explicit lifecycle behavior. An unavailable source is not silently
replaced with keyboard input. Keyboard-only implementation does not satisfy this
input scope.

The kernel already defines physical touch, pointer and raw-HID events with
device/contact metadata. Browser adapters must preserve those semantics and
enter the common binding/device routing; they must not disguise touch/HID
reports as keyboard events. The current BrowserGame keyboard entry point with
a fixed device identity is a compatibility path, not the full input contract.

Only browser-required host work stays on Window: event registration, permission
and user-gesture activation, minimal lifecycle control and resize/surface handoff.
Idle/setup/accessibility DOM updates are retained and event-driven. Continuous
game/audio message orchestration should remain with Worker/Worklet wherever the
browser permits, preserving actual output evidence, command ACKs and joined
cleanup. This rule concerns application work; browser-internal DOM paint is not
claimed absent. Measure input delay and main-thread workload before claiming
performance acceptance.

Canvas drawing and local/replay score HUD presentation run in Worker. Window
no longer duplicates their continuous score/status and song-position DOM writes
on step/render responses; final summaries and preview/menu updates remain
event-driven. Saved/network opponent DOM presentation and parts of the host
bridge remain to be moved; touch/pointer/HID input routes also remain to be
implemented and verified. Source changes do not establish performance acceptance.

## Finite live section controls

Live start and optional end use original-song decimal seconds with at most nine
fractional digits. A blank end retains full-song playback; a configured end must
strictly follow the start and fit signed 64-bit nanoseconds. Snapshot both before
opening audio, retain drafts after Stop/failure, and lock edits with existing
preparation/playback/import/record operation gates. Replay uses recorded section
metadata and ignores live section drafts.

Finite live setup must call the explicit BrowserGame section constructor backed
by StepGameplay. Worker and Window validate actual end/frame getters against
the requested section, prepared start, actual output rate and 100 ms preroll
before transferring samples. Missing/mismatched finite metadata fails setup with
no unlimited retry. Actual section completion uses the common logical/render/
presentation/command evidence and existing lifecycle barriers, then displays
Section completed. A capture marked complete means its configured section
finished; Stop/failure remains a prefix. Its canonical bytes retain the end.
Pressed-lane display stops adopting input suppressed by the actual endpoint
report. These source contracts still need real browser/audio acceptance.

## Recorded finite endpoint forwarding

For finite replay playback, the Worker must read the actual replay owner's
`end_ns` and `playback_end_frame` getter properties once and forward both in
preparation metadata. Unlimited replay omits both fields. Worker and Window
independently validate the paired BigInt values against the original prepared
start, actual output rate and 100 ms preroll: the frame fence equals
`ceil((end - start + 100000000) * rate / 1000000000)` and its rounded output
timestamp fits signed 64-bit nanoseconds. Missing pairs, nulls, invalid types,
overflow and mismatched fences are refused before audio sample transfer.

The Window retains that setup snapshot and calls `AudioHost.finish(endFrame)`
for finite replay; unlimited playback retains the argument-free `finish()`.
Finite setup failures use existing session cleanup without an unlimited retry.
Live playback requires actual finite preparation metadata to agree with its
requested endpoint; unlimited requests refuse unexpected finite metadata. Protocol fixtures
are authored for later execution; forwarding does not prove browser output.

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
the compatible HTTP/3 relay is also source-integrated. Ranked browser competition
remains follow-on work. Source integration does not complete or validate the requested player.

## Output context preferences

Live and replay output share retained controls for an interactive (default),
balanced, playback or custom latency hint and an optional requested context
sample rate. Custom latency accepts 0–60000 milliseconds with up to six decimal
places, without signs, spaces or exponents; category modes ignore inactive custom
text. The rate is automatic when
blank, otherwise a positive decimal integer fitting u32. One immutable snapshot
is admitted before asynchronous audio setup; AudioHost independently validates
and clones its optional context options before constructing AudioContext.
Unsupported requests fail explicitly without silently retrying defaults.

Both modes prepare PCM and run against the actual opened context rate. Output
preferences do not rewrite recorded judge timing, section or input. Busy owners
lock the controls, and failure or Stop retains page-local drafts. The
[Web Audio context options](https://www.w3.org/TR/webaudio/#dictdef-audiocontextoptions)
provide browser preferences: a latency hint is not a promised callback buffer
size or measured acoustic latency. Native backend/device selection remains in
the native adapter. Authored fixtures and source integration alone do not prove
browser support or playback behavior.

## Bounded output capacity settings

Live and replay launches admit one immutable set of output limits before audio
setup: command queue 1–65536, active voices 1–4096, pending commands 1–4096,
maximum render frames 1–4096 and command drain budget per render 1–65536.
All defaults are 4096. Values use unsigned decimal integers without signs,
spaces, exponents or fractions. Busy owners lock the retained controls;
failure or Stop retains page-local drafts. These are allocation and processing
limits, not a requested hardware buffer or guaranteed browser callback size.
The frame and pending ceilings match the existing Rust browser report contract.

The same snapshot reaches AudioHost. Its queue capacity also bounds the genuine
Worker command batch: min(256, queue capacity). Worker independently admits
that optional per-owner limit before chart preparation and applies it to both
setup replies and active command pushes; omitted limits retain 256. Actual
batch length must fit the admitted limit. Existing sequence and successful or
rejected-prefix acknowledgements retain their meaning. No retry, dropped prefix,
new clock or independent judgment logic is introduced.

A limit too small for an actual chart or browser callback can fail explicitly.
The Worklet does not drain commands before the armed start, so initial queued
BGM must also fit the chosen queue. Smaller batches do not prove that every
workload fits every capacity. Source fixtures remain authored and unexecuted.

## Finite practice output component

Browser finite practice is developed in dependent stages: exact output fence,
common stepped gameplay/recorded section completion, then Window/Worker end
controls. Output component support alone does not implement the user-facing
finite practice feature. The current page still launches unlimited output.

WorkletAudioBuilder::finish_at admits an immutable optional exclusive Mixer
playback frame endpoint; ordinary finish retains unlimited behavior. AudioHost
and the actual Worklet finish protocol accept an optional u64 BigInt endFrame,
validate it independently and forward it to the actual BrowserAudio finite
builder method. Omitted fields preserve the old call. Invalid values or a
failed finite binding do not silently fall back to unlimited output.

Arm checks absolute start plus the endpoint for overflow before mutation. The
Mixer emits the active prefix and silence after the exact endpoint, preserving
the immutable marker and callback chronology. Its relative marker is measured
after Worklet prestart silence; the absolute context endpoint adds the armed
start once. Zero is a valid output-component fence. User practice sections
still require a strictly later logical end when their controls are connected.
No UI clock authorizes a cutoff. Genuine Mixer and host-boundary fixtures are
authored; runtime execution and full finite-game/replay acceptance remain pending.

## Configured finite output evidence

Finite output admission uses an immutable configured relative Mixer endpoint,
never an endpoint inferred from a report. The ordinary decoder and shared
validator retain unlimited behavior. Their section-aware counterparts accept
only nonempty reports whose active prefix, frozen playback cursor, paused flag
and retained physical marker exactly match that configured endpoint.
For a physical block beginning at F with N frames and configured end E,
playback begins at min(F,E) and advances min(N,max(E-F,0)) frames. At or after
F+N reaches E, the report is paused and retains marker E. Zero is a valid
component endpoint. Empty finite snapshots are not new playback evidence;
callers use absent evidence instead. A zero sample rate is rejected.

The context cursor continues on the physical grid after the fence. Actual live
voices and pending commands may remain frozen at the fence; they are not a
full-song drain barrier. Later reports cannot change that retained state or
non-render counters. Clock, capacity, overflow and failure evidence checks still
apply. This evidence component does not yet connect finite gameplay, recording
metadata or page controls, and does not authorize completion by itself.

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

## Live saved-opponent host flow

The existing browser page supports explicit Own/Other comparison selection from
local stored records and imported replay files, with at most eight recordings
and a 64 MiB aggregate budget. Individual removal and clearing release the
selection budget. Selections retain immutable Files for retry; live preparation
checks each actual byte extent and canonical compatibility before activation.
Bad selected records fail preparation explicitly. Owner stamps fence stale
loads/read results. Replay playback ignores comparisons and remains local.

Actual BrowserGame snapshots publish saved-prefix counters at most four times
per second, correlated to the live play owner and actual game song frontier.
Comparison errors stop only comparison publication, preserving local judging,
audio, capture completion and optional multiplayer. Saved counters are distinct
from peer-reported network data and do not authenticate a result. No recording
bytes or chart assets are uploaded to the relay. Source integration still
requires later browser/bindings/audio execution evidence.


## Adjustable live judge timing

The browser live host exposes separate early/late windows and signed input
offset as retained millisecond text drafts. Defaults are 50 ms early, 50 ms late
and zero offset. Parse bounded decimal milliseconds with at most six fractional
digits directly through integer arithmetic into nanoseconds: early/late are
nonnegative signed-i64 values, offset spans the full signed-i64 range. Reject
exponents, whitespace, malformed text, excess precision and range overflow
without rounding. Capture one immutable timing snapshot for each live launch
before asynchronous preparation; preserve the original audio resume gesture.
Disable edits while the existing import/preparation/play/record-load owner is
busy, and retain drafts after failed preparation or stopped playback.

The Worker independently validates the requested BigInt timing fields before
chart preparation, defaults only an absent configuration, and forwards the
actual values to BrowserGame's existing constructor. Constructor/setup refusal
uses the existing cleanup flow. Recorded replay playback ignores these live
fields and retains the recorded judge. Saved-record compatibility and live
multiplayer identity use the genuine resulting profile through existing shared
logic; no new timing schema, rounding, judging implementation or clock source is
introduced. Authored fixtures and source integration are not browser execution
or timing acceptance.


## Original-source live section preparation

A fresh live section starts at a nonnegative original-song timestamp. Select
chart objects and overlapping BGM from the original prepared chart and decoded
PCM through the same section_start preparation as native play. Exclude earlier
heads and whole holds crossing the boundary; preserve original absolute targets
and timing markers. Crossing music selects the checked original-source frame
using the existing Ceil policy, never a previously selected suffix. Fresh starts
may reopen output and are not a gapless or acoustic synchronization guarantee.

The stepped live owner accepts only already-selected section data. Its initial
transport position is start minus preroll. Map original BGM onto output by
subtracting start exactly once and adding the existing output origin/preroll
exactly once; input keysounds still follow actual output scheduling. Clock
activation and presentation discipline retain this section anchor. Capture and
competition headers encode the immutable section start through the existing
replay profile, and genuine reconstruction validates the same section identity.
Natural completion requires original judge deadlines and real Mixer/presentation
drain, never a duration timer. Zero-start callers preserve existing behavior.

Browser preparation/binding and Window/Worker launch forward and validate the
same immutable start through the portable section owner. This flow is source-
integrated; compilation and authored fixtures do not establish browser or native
timing acceptance.


### Window/Worker live section launch

Expose retained Start time in seconds, default zero. Use the existing exact
seconds parser (at most nine decimal places and its signed timestamp range with
reserved lookahead), then snapshot a nonnegative BigInt start before audio
preparation awaits. Busy controls lock the draft; retry keeps it. The Worker
validates independently and prepares nonzero sections from fresh original
assets through BrowserLibrary.prepare_chart_at and section_start::prepare_at.
Zero uses existing preparation. Validate the actual prepared start against the
requested live snapshot before admission/activation, and verify it again in the
Window prepared response. A recorded replay ignores the live draft and uses
its decoded section start.

Original decoding retains the existing 1296-sample cap. Section preparation can
add at most 4096 overlapping BGM suffixes, so the output host admits a bounded
5392 samples while retaining its existing 64 MiB asset and 256 MiB aggregate
PCM budgets. No full-buffer copy is added at launch. Original-song graphics,
actual section-aware capture/competition identity and existing cleanup/drain
remain authoritative. Browser source integration still requires later generated
bindings and actual execution to prove acceptance.


## Retained keyboard bindings

Expose one retained key-choice draft per supported lane with Unbound and Reset
defaults. The catalog uses known physical KeyboardEvent.code names, as defined
by the [W3C code vocabulary](https://www.w3.org/TR/uievents-code/). Keep the
existing default code/physical-ID pairs stable; additional choices have explicit
application-specific source IDs, independent of native OS scancodes. Escape
remains Stop. Browser and OS shortcuts may prevent event delivery.

Snapshot and validate at most 18 unique lane rows before any live audio resume
or preparation await. Unknown codes, duplicate bound keys/lanes and malformed
rows refuse atomically; Unbound rows produce no key pair. Every actual prepared
lane must be bound. One immutable selection drives both the Worker request and
the Window event.code Down/Up lookup and displayed mapping. The existing actual
Worker and BrowserGame remain authoritative for binding admission.

Controls are created once and locked during import, preparation, playback and
record-store operations. Drafts, including editable invalid selections, remain
after setup failure or Stop; Reset is idle-only. Replay ignores live binding
drafts and uses recorded input. Single-keyboard play does not require a device
choice. Recording, comparison and network identity reuse the existing common
logic; no extra per-frame update, input clock or judging path is added. Drafts
are page-local, with no saved-profile claim. Authored fixtures and whitespace
checks do not prove browser/input/audio acceptance.


Finite replay setup now has a section-aware logical API preserving original
start/end and branch seed. Browser replay preparation and ownership use the section-aware APIs;
Window/Worker forward recorded finite endpoints into AudioHost setup. Live
start/end controls now use the common finite owner and record section metadata;
actual browser acceptance remains pending.


## Finite stepped replay owner

The portable stepped replay owner accepts recorded finite sections through
explicit section-aware preparation, visual reconstruction and audio planning.
Its immutable output endpoint uses the shared start/preroll/ceil-frame mapping.
Finite output is validated against that configured endpoint. Presentation
advances only recorded operations and clamps visual song position to the
original-song end; no terminal operation is synthesized.

Completion requires a retained actual Mixer fence, actual presentation crossing
the rounded endpoint, finished records, acknowledged command batches and retired
feeder credits. Actual consumed/applied counts must equal the admitted plan;
a late ACK cannot prove execution after the fence. Frozen live voices are
allowed at the fence. A missing or late
command still fails; silence alone cannot authorize completion. Browser replay
bindings retain and expose the endpoint and admit finite report telemetry.
Window/Worker forward and independently validate that recorded endpoint before
sample transfer, then configure the actual finite output component. Live
start/end controls use the same configured output contract. Browser execution
remains unverified.


## Common physical browser input API

Provide an explicit physical-binding constructor beside the compatible keyboard
constructors. Binding rows retain an exact 64-bit device selector or Any and
HID/native/vendor control namespaces; cover every prepared lane and reject
malformed, duplicate or over-capacity setup before creating gameplay ownership.
Canonical bounded BKPI event blobs retain the actual physical variant, source,
sequence, acquisition clock and native provenance. Reject malformed input and
unsupported clock domains before entering the shared gameplay path. Configurable
encoded/payload budgets must remain bounded; no Worker-arrival retimestamping.

Raw HID requires a device report adapter, and touch requires deliberate
interaction mapping. Merely accepting these events does not prove playable
hardware support. Pressed-lane presentation must follow actual admitted binding
reports and finite-end suppression. Browser permissions, hardware acquisition,
touch contact policy and application setup UI remain further integration work.

Physical setup rows contain seven unsigned 32-bit words: BMS lane, selector
(0 Any or 1 Exact), device low word, device high word, control kind (0 HID usage,
1 native, 2 vendor), page/namespace, and usage/code. Any requires zero device
words. HID page/usage fit unsigned 16-bit values; native/vendor namespace and
code retain all 32 bits. Exact device IDs combine both words without floating
point conversion. Setup admits at most 256 rules, permits ordered fanout, and
uses common exact-over-Any selection. Encoded input budgets are configurable up
to 1 MiB, with independent payload budgets no larger than the encoded budget.

Button highlights use existing bounded PressedKeys ownership and actual
bound-input reports, including committed partial reports. Releasing one source
cannot erase another source holding the same lane. The finite endpoint clears
all highlight ownership. Any presentation observation failure after a committed
report preserves judgment progress and explicitly fails the owner; it cannot
roll back judgment or replace the primary runtime/capture failure.

Input admission and replay capture have independent byte budgets. A blob
admitted by a larger configured input budget can still exceed capture limits;
that failure must retain the committed runtime report and fail explicitly.
Existing capture stores admitted bound GameInputEvent values, including
physical metadata and control identity, rather than rerunning device binding.
Unbound raw reports are not automatically captured as judged gameplay input.


## Page physical-input routing

Live page setup must explicitly request the physical input route and require
the same route in preparation metadata before transferring samples. The replay page
omits live input route choices. Worker compatibility requests may keep the
legacy keyboard path; explicit physical requests must fail on missing APIs
instead of silently choosing legacy constructors.

The Window acquires keyboard transitions; the Worker encodes their complete
bounded batch into canonical BKPI button events before the first runtime call.
Preserve acquisition time, sequence and output scheduling. Historical browser
key IDs are adapter codes, not USB HID usages: physical routing uses Native
controls in browser keyboard namespace 0x574b4559. Source 1 represents the
Window keyboard aggregate; it does not distinguish physical keyboards.
Original acquisition clock provenance uses the actual Window HOST domain
0x57494e and event timestamp; Worker receipt time never replaces it.

Keep one consuming setup owner, actual finite/unlimited metadata, pre-origin
filtering, monotonic input watermarks, audio ACKs and joined stop/capture barriers.
Touch/HID acquisition and application device permissions remain further work.

No-note charts with no prepared lanes may use an empty physical binding set,
preserving the existing all-Unbound page configuration. Empty bindings must
still validate input budgets and must fail when any prepared lane needs binding.
Do not invent a control or fall back to legacy ownership to admit such charts.

## Input acquisition ownership and contact mode

Window collects keyboard, touch/pointer and HID input with original timestamps,
sequence and source/contact identity. Worker owns continuous judging, gameplay
state and rendering; Worklet owns audio. Window also handles browser-required
permissions, user gestures, lifecycle and surface setup. Device support remains
subject to browser capabilities; acquisition does not itself imply playable
bindings. Touch must retain real contact events, never fake keyboard events.

An explicit physical contact constructor selects the BMS button/contact rules.
Existing physical keyboard and legacy constructors keep button-only semantics.
The selected mode must survive capture and typed replay reconstruction. Browser
touch acquisition/lane routing and HID permission/report adapters remain pending.

## Prepared physical touch regions

Contact-mode BrowserGame can configure bounded seven-word physical identity
rows plus four finite rectangle bounds per row before activation. Regions may
cover a subset of prepared lanes alongside keyboard controls; empty regions
are valid. Reject destinations outside the prepared chart and inconsistent
row/bounds extents. Reuse the common physical identity decoder and TouchRouter.

A projected touch packet entrypoint accepts genuine canonical Touch events and
separate finite hit coordinates, then uses the same StepGameplay/Runtime report
and capture path. Keep original event coordinates, time and provenance.
Window/Worker acquisition, layout projection and contact presentation feedback
still require integration before playable browser touch is claimed.

## Window touch acquisition and Worker bridge

The live touch policy auto-enables on touch-capable PointerEvent browsers and
may be selected before play; keyboard input remains available. This selects
contact judging, not a physical device picker. Preserve the explicit mode in
preparation, recording and competition identity. Different legacy/contact
identities are not silently made compatible.

Window collects actual touch pointer events on the canvas with acquisition
timestamps, contact nonce, pointer ID provenance, original canvas-relative CSS
coordinates/pressure and cached CSS extent. Do no rendering, region hit testing,
canonical serialization or per-event DOM geometry query there. Capture the
pointer until matching Up/Cancel; unexpected capture loss becomes contact Cancel
at its actual event time. Browser focus/page loss still stops playback.

Worker validates and serializes the entire bounded mixed keyboard/touch batch
before the first gameplay call. Project hit coordinates separately using actual
logical canvas dimensions and configure regions from the same lane partition
used by rendering. Keep physical coordinates/contact/times unchanged in the
canonical Touch packet and actual runtime/capture report. Never fabricate keys.

Missing PointerEvent/capture support or missing contact binding capabilities
must fail explicitly before consuming preparation or audio data where possible.
Retain pre-origin filtering, monotonic watermarks, finite-end output and command
ACK/stop/capture barriers. Source/compile checks are not browser/device acceptance.
Contact pressed feedback, WebHID and cross-mode record/native competition
integration remain separate pending work.

## Admitted contact pressed feedback

Visible pressed lanes derive from actual admitted bound input, independently
of hit or miss judgement. Preserve separate button and contact ownership using
full source, physical surface/control, game control and contact ID. Touch Down
adds one owner; duplicate Down is idempotent, Move does not acquire or relocate
it, and matching Up/Cancel removes only that owner. Unknown releases do nothing.
A button and contact, or two contacts, may share a lane; it stays pressed until
the final matching owner releases. Never synthesize keyboard events or re-hit-test
raw touch coordinates for presentation.

The existing common bounded ownership component supplies browser live feedback,
native member publication and recorded replay presentation. Whole input batches
retain atomic capacity refusal, and clear releases all visual ownership while
retaining reusable storage. Playback-prefix presentation follows only recorded
operations and must match live ownership transitions. Compilation and authored
fixtures do not establish actual browser/device acceptance.
