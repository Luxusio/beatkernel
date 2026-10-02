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
Runtime and separate AudioWorklet Mixer. Output presentation discipline, complete
result/capture/replay integration and browser network transport remain follow-on
work. Source integration does not complete or validate the requested player.

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
Actual getOutputTimestamp presentation discipline and complete
result/capture/replay integration remain unfinished. Browser input timestamps use
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
drift. The AudioHost outputTimestamp accessor exposes genuine browser evidence,
but the player has not yet integrated that evidence into clock discipline.

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
available and cannot claim natural completion. Input/output drift discipline,
capture/replay, persistence and browser networking remain separate unfinished
work; this acceptance contract is not evidence that the source was executed.

A gameplay disposal error during completion or manual stop is retained as a
cleanup failure. The Window terminates that Worker and requires reload before
another play; an earlier natural-finish candidate does not hide the error.

The numeric Worklet-report decoder belongs to the existing portable application
audio boundary. WASM bindings delegate to its single implementation; direct
decoder fixtures can run as ordinary host Rust tests without generated bindings
or a browser. This changes no PCM callback or serialized byte format.
