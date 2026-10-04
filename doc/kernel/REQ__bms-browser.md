# Browser host and shared selected-file preparation

## Local gameplay binding boundary

The browser local gameplay binding consumes one live BrowserPrepared and the
portable resolved source plan. Numeric binding rows carry the player ID followed
by the existing seven physical identity words. Preserve member order and all
source bits. Limit the combined bindings to 256 rows; each member must cover the
prepared chart lanes. Multi-player mappings require their assigned exact source;
solo automatic mappings remain available without choosing a device.

Reuse StepLocalGameplay for every member, with one sample bank, output clock,
BGM producer and command/ACK owner. Keep per-member score, capture, contact
routing and retained note/pressed feedback. Decode canonical physical input using
the existing codec and preserve source/contact/acquisition provenance. Raw HID
uses the existing bounded profile adapter. No synthetic keyboard path or
per-player audio transport is introduced. Validate actual output evidence before
publishing report-driven BGM commands.

The Worker canvas exposes local drawing through the common four-field page
composer. Borrow actual member score, note progress and recent results instead
of cloning chart-sized progress or score maps each frame. Retain the shared
bounded image cache; select backgrounds from each visible member's own frontier.
Drawing neither advances judgment nor creates output evidence. Invalid page
requests refuse before surface recreation or resource synchronization.

Known ceiling: The browser page now connects local source assignment and
per-member record selection to the local Worker caller in source. This does not
prove browser execution, generated JS bindings, device coverage, shared audio
completion or performance acceptance.
The 64-member roster bound and 256 combined binding-row bound are independent:
a chart requiring many lanes can reach the binding budget before the roster
bound. Refuse incomplete member coverage rather than silently sharing sources
or truncating mappings.

## Local Worker caller

Live physical preparation optionally carries localPlanWords in the existing
four-word resolved-plan schema and an initial localPage. Snapshot numeric setup
before asynchronous reads. Build the eight-word per-player maps from actual
physical profiles, with source one representing the browser keyboard aggregate
and source two the touch aggregate. Do not split either aggregate into fictional
devices. Exact members receive only their assigned source's rows, and every
member must cover actual prepared lanes. Automatic solo needs no source choice.
Bound complete setup and preserve all original source bits. Refuse unsupported
profile/assignment combinations explicitly before consuming preparation.
Return only assigned Gamepad sources to the host for input/disconnect admission.
The host must filter unassigned samples before enqueueing a local input step.
An explicitly submitted unassigned changed sample still belongs to the Worker's
original acquisition chronology, even though the common runtime ignores its
source. It can therefore cause late-input refusal; never retimestamp it or invent
a separate deadline to conceal the caller's admission error.

The Worker uses one BrowserLocalGame, original sample stream, command endpoint,
report/ACK lane and completion barrier. Drawing calls the local canvas with the
current bounded page. Page changes are presentation operations, not game-clock
operations. Configure a selected touch member against its actual visible field
before activation; do not use solo full-size bounds. Keep original pointer
payload separate from projected routing coordinates. A touch member must be on
the initial page. Page changes remap only the configured geometry and new-contact
admission. Held contacts retain their original destination through release; a
hidden touch field admits new contacts as unbound until the member returns.
Page choices use a correlated RPC. An invalid page or a currently unavailable
touch-layout remap returns a page-choice error while preserving the running
game. Malformed owner/RPC identities retain the existing protocol failure fence.

Replies preserve each member's actual score/frontier independently, with null
for unreadable fields. Legacy top-level score fields identify the first member,
never a synthetic cohort total. Stop/failure exports each actual recording once
before free, with independent recording errors and transferred buffers. Bound
aggregate recording allocation/export budgets, preserve readable member prefixes
after other export failures, and claim complete captures only after genuine
shared completion. Direct port ownership, chronology, stale callbacks and joined
cleanup keep their existing barriers.

Known ceiling: The page device-assignment and individual record save/download
flows now call this protocol in source. Local saved comparisons now require an
explicit current member target for each selected record and use independent
member ownership; local network comparisons remain unsupported and refuse
explicitly without altering existing solo comparison behavior. Browser execution,
generated bindings, physical devices/audio and performance acceptance remain
unverified.

## Performance-first browser thread ownership

The Window main thread must do as little application rendering as possible.
Continuous gameplay rendering, including HUD and score presentation, belongs to
the graphics/game Worker through OffscreenCanvas. Do not assemble scenes, draw
gameplay canvases, update a DOM gameplay HUD each frame, judge inputs, decode
assets or serialize gameplay/network data on the input acquisition thread.

The Window collects supported keyboard, touch/pointer, HID, Gamepad and other
physical input events and forwards a bounded, ordered prefix through the common input
abstraction. Preserve acquisition timestamps, source/device/contact identities
and original clock provenance; Worker arrival time cannot replace event time.
Permission refusal, cancellation, disconnect, focus loss and released contacts
must have explicit lifecycle behavior. An unavailable source is not silently
replaced with keyboard input. Keyboard-only implementation does not satisfy this
input scope. The user's main-thread input restriction applies to keyboard,
touch/pointer, HID and Gamepad acquisition together; it does not narrow supported
input to keyboards. Each source follows the same acquisition/Worker ownership
boundary.

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
event-driven. Saved and network opponent presentation also have Worker-owned
source implementations; parts of the audio host bridge remain to be moved.
Keyboard and live touch forwarding have source implementations. HID acquisition,
profile launch and mixed report forwarding also have source implementations on
Window/Worker. Input route completeness, browser execution and measured latency
remain to be verified. Source changes do not establish performance acceptance.

## Worker-owned saved opponent presentation

Continuously changing saved opponent counters belong to the graphics Worker.
Reuse the common portable competition snapshot and scoreboard components;
retain a bounded snapshot of actual SavedOpponents prefixes at the existing
display cadence, independent of local judgement and audio callbacks. Rendering
reads this retained snapshot and never advances a replay or song clock.

Window receives no normal periodic saved-counter messages. A comparison failure
has one explicit notification, hides invalid HUD data and leaves local play
running. An independently failed comparison cannot become a successful final
result. On stop or gameplay failure, capture one final comparison prefix before
releasing the owner and carry it in the correlated stop/error receipt. Window
may display that final result after joined cleanup, once for the current page
and session. Preserve exact u64 counters, labels, kind and recorded prefix
extent; never fill an unrecorded tail or change local capture completeness.

The shared view retains actual admitted opponent identity and scores. Empty
selection, replay, unavailable comparisons and stale final receipts have explicit
behavior. Source fixtures and compilation do not establish rendering or
main-thread performance acceptance.

## Worker-owned multiplayer peer presentation

Retain the actual peer prefix and connection lifecycle on Worker. Coalesce normal
peer progress at the existing display cadence and update the Rust HUD through a
bounded numeric interface, preserving signed i64 song time and all u64 counters.
Use the common progress validator; do not infer a song clock, local judgement,
authenticated score or peer identity. The shared competition scoreboard labels
the peer's data as reported and displays its independent connection state.

Saved and peer presentation failures are independent: disabling one preserves
the other's retained snapshot. A presentation error cannot change local scoring,
capture completeness, transport framing or actual final write/ACK results.
Invalid HUD updates refuse atomically. No update revives a stopped/disconnected
presentation owner, and rendering never advances a network or replay clock.

Window receives setup/lifecycle/error notifications but no normal periodic peer
counter updates. Clock estimation and committed-start ownership remain on their
existing paths. Final peer progress goes immediately to the Worker HUD, not to
a live DOM counter. Stop/error receipts preserve the last actually received peer
prefix and whether it was a final prefix, independently of local final write and
peer ACK. Present that prefix once after joined cleanup for the current page and
session; an absent peer prefix remains absent. Stale callbacks cannot change a
new session or resurrect a disposed owner.

Source fixtures and compilation do not prove browser presentation, network
interoperability or performance acceptance.

## Dedicated Worker-to-Worklet command ownership

The continuous command path shall use a transferred MessagePort, avoiding
Window command validation, copies and per-batch ACK relay. Introduce the bounded
transport component before changing its gameplay caller. AudioContext creation,
user activation and actual output timestamp acquisition remain host operations.
The explicit low-level host polling path retains its own control sequence and
output evidence; the direct gameplay caller reads reports on Worker.

AudioHost.openCommandPort() is a once-only handoff after allocation and before
arming, with no pending host operation. It transfers one MessageChannel endpoint
to the actual processor using a correlated host control ACK, and returns the
other endpoint with generation, queue capacity and timeout metadata. Unsupported
MessageChannel, repeated handoff, wrong phase and failed attachment refuse
explicitly. After handoff, host commands() refuses; there is one command producer.
Setup/sample/finish/arm/poll/stop remain on the host control lane. No retry or
fallback resumes host command authority after a transferred attachment.

The processor command lane has an independent safe integer sequence, initially
zero, accepts bounded command batches and actual report polls, and preserves
the actual Rust enqueue
admitted-prefix count. Both lanes share the same structural command preflight
and enqueue operation. Generation mismatch, wrong operation, sequence gaps,
malformed records and failed admission fence the audio owner. A failed batch is
never retried, split, silently dropped or acknowledged as successful. Terminal
failure reaches both endpoints, while host stop remains the sole deallocation
route and explicitly closes the command endpoint.

The Worker-side AudioCommandClient owns at most one pending operation, validates
and snapshots the exact seven command fields, correlates generation/sequence and
full or rejected-prefix ACKs, and has a bounded timeout. It preserves lossless
BigInt command fields and returns actual admitted evidence. Malformed response,
terminal, message error, timeout or explicit close permanently fences this client,
settles its pending promise and detaches/closes its port. No close, late message
or old generation can revive a disposed owner. Closing the client does not claim
that the AudioContext or Rust processor has been released; joined host cleanup
still owns that evidence.

This transport component is consumed by the direct gameplay caller described
below. Source integration does not prove browser execution, input delay, real
audio or main-thread performance. Existing output/presentation and completion
barriers remain required.

API basis: the [HTML channel messaging specification](https://html.spec.whatwg.org/multipage/web-messaging.html#channel-messaging)
defines transferred endpoints and queued message delivery; the
[Web Audio specification](https://webaudio.github.io/web-audio-api/#dom-audioworkletprocessor-port)
defines the node/processor MessagePort and recommends explicit port closure.
These API contracts do not establish implementation or performance acceptance.

## Direct gameplay command caller

The page must obtain the real AudioHost command descriptor after finish and
transfer its port exactly once to the current gameplay Worker before arming.
Both live and replay use the direct path. The Worker owns AudioCommandClient,
drains actual initial core commands through it, and only replies to setup when
all actual admitted-prefix ACKs have reached the core. A preparation error,
stale page/session, failed transfer or cancellation closes any endpoint still
owned locally and enters joined cleanup; no host-command fallback is permitted.

During play, commands flow from the actual BrowserGame/BrowserReplay batch
through AudioCommandClient directly to the processor. The returned transport
sequence is distinct from the real core batch sequence; the retained core batch
is acknowledged only with its own identity and actual admitted count. A rejected
prefix is acknowledged as failed before fencing the game where possible, with
no retry, dropped tail or invented success. ACK errors, timer expiry and late
responses cannot mutate a freed or newer game. Close/detach the client before
freeing its gameplay owner, while AudioHost still joins actual processor/context
cleanup independently.

Window must not receive normal command arrays or relay per-batch ACKs. It retains
actual poll/output-presentation observations and bounded step/render protocol.
Step responses carry the actual commandsPending boolean; render responses carry
commandsPending and observedTick from the Worker input prefix. Window accepts
natural completion only when observedTick equals its latest issued tick, its
input/render operations and event queue are drained, and no command prefix is
pending. Stale completion cannot authorize natural stop after newer input. Setup/ready and output
completion barriers remain actual core evidence. Legacy lower-level Worker
control callers may retain their explicit untransferred command route; the
player page always uses the new handoff and cannot silently choose that route.

Source fixtures must cover real client/Worker ACK correlation, initial drain,
active live/replay commands, rejected prefixes, pending completion, stale
callbacks after cancellation/free and one-time page endpoint transfer. Browser,
physical audio, generated bindings, input latency and performance remain
unverified until their deferred execution and QA are performed.

## Direct report transport component

Extend the transferred endpoint with AudioCommandClient.poll() before migrating
its gameplay caller. Commands and poll share one monotonically ordered transport
sequence and at most one pending operation. Overlap refuses locally; no hidden
queue, polling timer or command retry is introduced into this transport client.
Successful poll returns the actual processor report, independently of command
admission; rejected poll must report zero admitted commands. A poll response
cannot settle a command request, or vice versa.

The Worklet reuses one control-handler report builder for both host and direct
poll. It reads the actual BrowserAudio report_word fields, preserving all 56 u32
words and their availability marker, with no simulated progress or timer-derived
cursor. Report allocation stays outside process(). The direct endpoint accepts
only commands and poll and preserves generation, ordering, phase and terminal
fences. Host polling remains explicitly available until caller migration.

The client validates the successful report shape: Uint32Array length 56, ordinary
fixed ArrayBuffer backing of exactly 224 bytes with zero offset, availability
word 0 or 1, reserved header word zero, and boolean available matching that
marker. Detached, shared, resizable, truncated, misoffset or contradictory
reports permanently fence the client rather than becoming output evidence.
No report is actual rendering evidence before available becomes true. Full
report semantic validation still belongs to the existing Rust output decoder
and gameplay owner; this shape guard does not replace it. Non-poll and rejected
ACKs cannot carry a report. All poll error, timeout, close and stale-message
behavior obeys the same bounded owner lifecycle as command operations.

The direct gameplay caller described below consumes reports on Worker. It
preserves original Window output-presentation observations and input clock
provenance, serializes report operations with actual command submissions, and
retains completion/cleanup barriers. Browser execution, actual audio and
input/render/main-thread performance remain unverified.

## Worker-owned direct report caller

The player page sends a bounded play-render observation containing its render
identity and original raw timestamp/observedNowMs values, with no report payload.
Worker projects them to the presentedNs/presentedHostNs pair described below.
Window must not call AudioHost.poll() or relay Worklet word arrays. Window retains
AudioContext output timestamp acquisition, its actual performance domain and
one pending render deadline. Missing presentation remains null; Worker arrival
time cannot substitute for original output or input evidence.

The attached gameplay Worker reads the real report through AudioCommandClient
poll() and passes its actual words through the existing renderedCursor and Rust
observe_output/observe_presentation path. Commands and report reads share one
bounded operation owner, with no simultaneous client requests or hidden queues.
One pending render observation is allowed; repeated, overlapping, stale or
out-of-order observations refuse. A pending command batch receives its actual
ACK before report polling; an active report waiting behind it is serviced before
new command batches so report-driven BGM credits cannot starve indefinitely.
Initial setup still drains genuine commands before readiness and activation.

Direct mode refuses externally supplied report arrays; the explicit unattached
low-level Worker protocol keeps its existing report API. Report timeout, bad
shape/semantics, terminal or generation failure fences the gameplay owner.
Polling never acknowledges a core command or fabricates rendered progress.
Original presentation pairs remain unchanged across awaits. The output report
is processed against actual current core state and input chronology, not an
old pre-await game snapshot.

Step/render replies retain actual commandsPending and observedTick. Report
operation state is distinct from command admission state; neither async work
nor a stale completion may permit natural stop. A report's processed request is
retired before publishing completion, and newly generated commands are accounted
for before the completion snapshot. Stop/failure clears pending observation,
closes/detaches client before freeing the game and guards late poll/ACK callbacks
against stale or freed ownership. Joined actual AudioHost cleanup remains required.

Both live and replay page callers use this direct report path with no fallback.
Source fixtures must cover serialized report/command ordering, real report and
original presentation provenance, pending completion, refused overlap/external
payloads, failed or cancelled reports and stale owner callbacks. Browser/device/
audio execution and measured input/render/main-thread performance remain deferred.

## Worker projection of original host observations

Window acquires AudioHost.currentFrame for a live input batch and sends the
unchanged u64 contextFrame instead of computing relative audioNs. The shared
pure audioScheduleFromFrame(contextFrame,startFrame,rate) validates both u64
frames and the actual u32 rate, adds the existing ceil(rate/50) frame lookahead
with checked u64 range, clamps pre-start position to zero and uses frameNanos
for the original exact floor conversion and signed-nanosecond bound. Worker
runs this projection before any gameplay input mutation. Acquisition timestamps,
input ordering and offset application remain unchanged.

Window sends play-render with a raw timestamp snapshot containing the actual
AudioContext contextTime and originating performanceTime, plus observedNowMs
from the same Window performance domain. Unsupported/unavailable presentation
is explicit null. No Worker arrival time, alternate clock origin or estimated
presentation replaces these values. Worker snapshots and validates the numeric
fields before awaiting direct report polling, projects them with the existing
presentationPair helper and retains the former per-session monotonic guard.
Regressing/stale/future/pre-start observations yield the same absent evidence;
a repeated output position cannot refresh retained observation age. Malformed
observations refuse before report admission or core mutation. Valid projected
pairs pass unchanged into the existing Rust output/presentation evidence path.

The actual page sends only raw frame/presentation observations. Explicit
lower-level Worker callers may retain the previous audioNs or projected-pair
API; raw and projected forms are mutually exclusive and no page failure silently
falls back to a projected value. Render identity, pending command/report and
latest-input completion barriers remain unchanged. Closing a session clears its
retained presentation state; late observations cannot update a newer owner.

Deferred source fixtures must prove exact long-duration/overflow frame projection,
original Window timing across different Worker origins and awaits, equal-output
age behavior, malformed/mixed-form atomic refusal, actual scalar page packets,
and live/replay cancellation/completion regression. These source changes do not
establish browser/device execution or measured performance acceptance.

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
controls. These source stages are connected for finite live and recorded replay
sections: the page forwards the actual configured frame endpoint to the output
owner. Ordinary playback remains unlimited. Browser execution and exact physical
output acceptance remain unverified.

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
source forwards a logical keyboard, live pointer/touch contacts and optional
profile-based authorized HID interfaces through common physical input routing.
Bindings, source/contact identities and acquisition provenance remain explicit;
actual device execution and route acceptance are unverified.

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


## Bounded room-frame byte transport

The reliable WebTransport channel retains its 65547-byte default prefix limit.
An explicit `maxPrefixBytes` option may select 1..65808 bytes, including the full
BKMR room admission frame. Validate it before acquiring a transport and enforce
it on read prefixes and owned write snapshots. Keep the fixed 1 MiB platform
chunk/backing-buffer retention limit, single pending read/write ownership and
existing cancellation/deadlines. The channel remains a byte adapter; Rust owns
room interpretation, leases, preparation and complete-write receipts.

This limit extension is a component for future browser room integration.
Generated bindings and a Worker-owned room client are still required; it does
not select group mode automatically or prove browser/server interoperability.

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

## Optional WebHID acquisition boundary

The [browser HID contract](REQ__browser-hid.md) defines bounded permission,
actual report snapshots, exact separate-ID payloads, source identity and
asynchronous owner cleanup. Window only acquires; canonical raw packet
encoding and report interpretation belong off the main thread. This component
does not yet claim live page forwarding or playable lane bindings. Those
integrations remain required work.

The Rust live binding configures complete HID source profiles before activation
and ingests canonical raw packets into the existing gameplay/report pipeline.
Control bindings are supplied during construction, not changed during play.
The common HID decoder and acquisition-order validator remain shared with
native adapters. Window/Worker profile forwarding remains separate integration
under the [HID contract](REQ__browser-hid.md).

Worker accepts optional bounded HID setup alongside live physical keyboard or
contact input, configures the actual Rust owner before publishing preparation,
and forwards genuine HID reports through the same ordered mixed input batch.
HID bindings can supply prepared lane coverage without keyboard mappings.
Window permission/profile selection and launch forwarding follow the page
lifecycle contract below; actual execution remains unverified.

The page optionally authorizes HID from an explicit gesture and discovers
authorized interfaces automatically for live play. It forwards profile files
and acquired device identities to Worker, where parsing and eligible-device
matching occur. Preparation proves admitted sources before Window forwards
reports. All stop paths detach listeners and join HID ownership cleanup. The
[HID contract](REQ__browser-hid.md) defines profile version 1 and refusal rules.


## Gamepad acquisition component

Physical input scope includes keyboard, touch, HID, gamepad and other supported
adapters. Window performs only browser-required acquisition and lifecycle work;
Worker owns control interpretation, bindings, judgment and rendering.

The optional `GamepadInputOwner` component acquires bounded button/axis samples
from the Window Gamepad API. Explicit polling has no private render loop or
timer. Preserve browser-normalized double values, pressed/touched flags, mapping,
index and product description, and the original `Gamepad.timestamp` in the
Window clock domain. Do not substitute poll or Worker arrival time. Equal
timestamps may contain changed samples and must not alone suppress acquisition.

Use the caller's shared source allocator for connection-scoped source IDs,
independent of browser index and product description, and the shared acquisition
sequence allocator; source and sequence allocations must advance within the
owner and remain bounded u64 values. Connection continuity uses the actual
Gamepad object, with index only locating its slot. Replacement objects, absent
slots and observed lifecycle retirement terminate the old source; a stale
disconnect for an old object must not retire a replacement. Never derive HID
usages or hardware serial identity from Gamepad metadata. Retirement reports the original source without fabricating a
release. Limits and whole-poll validation bound storage and reject malformed
samples before publishing a partial poll. Close and failure detach listeners
and fence further publication, including callback reentry. Lane bindings,
axis thresholds and duplicate suppression belong to a later Worker adapter.
Defaults and hard caps are 16 active devices, 64 slots, 128 buttons and 64 axes
per device, with product descriptions capped at 1024 code units. Smaller
positive limits are configurable. Invalid or exhausted allocators, native
errors and consumer exceptions permanently fail the owner. Whole-poll
validation prevents malformed-tail publication; a consumer exception cannot
roll back callbacks already delivered.

The [W3C Gamepad specification](https://www.w3.org/TR/gamepad/) exposes Gamepad
to Window, defines its timestamp as the latest browser update, allows index
reuse after disconnect, and defines product descriptions without unique device
identifiers. This API supplies snapshots, so polling cannot recover transitions
that occurred between observations. Browser exposure/permission and lifecycle
behavior require actual browser/device checks.

Known ceiling: Window acquisition and Worker canonical forwarding have source
implementations, including the automatic solo session described below. This
does not establish actual browser gamepad execution or capture/replay acceptance,
perfect reconnect identity when browser lifecycle evidence is missing, or
measured latency. Deferred fixtures are source only; execution remains pending.


## Worker Gamepad profile and canonical ingestion

Optional live physical setup accepts `gamepadSetup` with at most sixteen
connection-scoped device descriptors (`source`, `buttons`, `axes`) and at most
256 five-word binding rows: lane, source low word, source high word, control
type and index. Types are pressed button (0), absolute stick axis (1), analog
button value (2) and touched button (3). Sources are distinct u64 identities
at least three and must not overlap admitted HID sources. Controls must fit
the configured device's button/axis counts. Worker snapshots setup before
asynchronous acquisition and uses exact source native bindings in backend
`0x57475044`, with control codes respectively index, `0x10000 + index`,
`0x20000 + index` and `0x30000 + index`. Pressed and touched button bindings satisfy
ordinary press-chart lane coverage. No implicit axis-to-key threshold exists.

Worker retains bound control levels per source and emits canonical BKPI Button
and absolute Axis events through the existing Rust `input_blob` entry point.
Pressed/touched transitions remain buttons in their own native namespace.
Analog values remain Axis events. Acquisition snapshots retain browser doubles;
canonical axes use the shared core's explicit float32 representation. Initial
false button levels emit no release; initially true levels emit Down. Changed
values at equal sample timestamps remain valid. One sample's fanout shares the
original source, Window time/domain, native provenance and acquisition sequence.

Preflight the entire step into bounded draft adapter state before calling
Runtime. Limit original samples and Worker-generated canonical packets to 256 per step;
raw HID report field expansion remains subject to its existing profile limits. Invalid
source identity, counts, values, setup or fanout fail explicitly with no partial
preflight mutation. Repeated state emits no events and may retain a sample's
old timestamp after the global watermark has advanced. A changed sample older
than that committed global prefix must fail instead of being retimestamped.
The source's original timestamp and sequence must still advance according to
its acquisition rules. A validated pre-origin sample updates retained source
levels but its events are not submitted to Runtime; a held pre-origin control
must not produce a synthetic later Down. Count pre-origin input once per
original sample, independent of fanout. Keep setup, timing, input and lifetime ownership on
Worker; Window remains responsible for browser-required acquisition only.

Known ceiling: Automatic standard solo Window forwarding is implemented in
source as described below. Explicit nonstandard page profile selection also has a source implementation
as described below. This does not prove browser/device/capture/replay acceptance. Axis bindings do
not imply support by ordinary press judgment. Late sampled changes are refused
by the existing committed-prefix contract; polling cannot recover unobserved
intermediate transitions. Browser input latency needs actual measurement.


## Automatic solo Gamepad page acquisition

For live solo play, discover browser-exposed Gamepads automatically when
`getGamepads` is available. Replay acquires no Gamepads. Window snapshots
connection descriptors and forwards them as optional `gamepadDevices`; this
is exclusive with explicit lower-level `gamepadSetup`. Worker validates at
most sixteen descriptors and admits standard mapping devices with at least
nine buttons. Default pressed-button indices 0–8 map to solo lanes 0x11–0x19.
Unrecognized layouts have no inferred bindings; explicit lower-level setup
remains available. This default does not implement local multiplayer device
assignment or claim support for every controller layout.

HID and Gamepad acquisition share a session source allocator starting at three.
The optional HID allocator preserves the old standalone sequential default.
All accepted identities are bounded increasing u64 values and allocations are
burned on failed setup. Keyboard remains source one, touch source two. Validate
Worker-admitted Gamepad sources against actual owned eligible descriptors
before output activation. New connections wait for a fresh session setup;
a participating disconnection stops the current session. Preserve native
sample timestamps and the common acquisition sequence, without Main-thread
control interpretation or additional render/poll timers. Poll as part of the
existing available live input pump. Pending input remains bounded at 1024
samples. Stop detaches owners before joining Worker/audio release; failed
cleanup requires reload. Constructor-time acquisition failure retains actual
cleanupError evidence if listener removal also fails, so Window can apply the
reload fence even before it receives the owner instance. Operational failure
remains distinct from cleanupFailure. Late callbacks cannot revive old sessions.

Mixed sources preserve their original acquisition identities and source order.
The shared core validates sequence per device, not across unrelated devices.
Worker preflights device-specific sample chronology, then stably orders actual
nonempty inputs by original acquisition timestamp within a step. Equal-time
inputs retain captured order. Source sequences and native provenance are never
rewritten by sorting. The committed global timestamp frontier still applies,
so changed late input fails explicitly; unchanged Gamepad samples do not move
that frontier. Commit bounded draft source/adapter state only after complete
validation. Main pending cursor/watermark must use the bounded batch's maximum
original time and prior cursor, never a stale last Gamepad sample.

Known ceiling: Actual browser/device execution and measured latency remain
unverified. Snapshot polling cannot recover intermediate transitions.
Nonstandard layouts have optional explicit profile support; automatic inferred
bindings and local multiplayer assignment remain work;
explicit configured axis events do not imply ordinary press-chart judgment.
The existing late-input refusal remains observable at the committed frontier.


## Optional Gamepad profile file

Window accepts an optional actual JSON profile File for live Gamepad play and
forwards it unchanged. It validates only the nonempty file's size (at most
1 MiB), retains its selection across stop and invalid replacements, and offers
explicit clear to restore automatic standard mapping. Selection/clear controls
are locked during active work. Replay ignores the live profile and performs
no device acquisition. An explicit profile requires Gamepad acquisition support.
File reading, parsing, matching and control construction belong to Worker.

Version 1 is strict UTF-8 JSON with exactly `version` and `profiles`. Profiles
number one to sixteen. Each profile has mandatory `bindingWords` (one to 256
complete three-word rows: lane, type, index) and optional exact `id`, `mapping`,
`buttons` and `axes` matchers. Unknown fields and coercion are refused. Product
descriptions are bounded to 1024 code units; mapping is empty or standard,
button count 0–128, axis count 0–64. Types are the existing pressed button,
absolute stick axis, analog button value and touched button namespaces.
Pressed and touched button bindings prove ordinary press-chart coverage. A
touched control uses its own true/false contact signal and native control code;
it does not alias the pressed field or synthesize keyboard input. This Gamepad
button contact signal is distinct from position-bearing PointerEvent touch.

Worker snapshots actual source descriptors before asynchronous reads. Every
profile is validated, including unmatched ones. For each owned source, zero
matches means ignored; multiple matches mean ambiguous and fail. No matched
source fails an explicitly selected profile. Checked controls use the actual
matched device's counts. Build exact-source binding rows retaining all 64
source bits and enforce at most 256 combined Gamepad rows. Several identical
products may share a profile while retaining separate runtime sources. The
product description is not a serial identity or a local-player assignment.

Read the actual File exactly once and require its returned fixed byte length
to match the snapshotted size. Ownership is checked after each await, so stop
cannot create a late game. Explicit profile plus descriptors is exclusive
with lower-level numeric `gamepadSetup`; no profile preserves automatic setup.
Window verifies a nonempty distinct admitted subset of its actual owned
sources before audio activation. It does not parse matchers to infer admission.
During explicit-profile preparation an owned candidate disconnect cancels
setup; after preparation only admitted disconnects stop play.

Known ceiling: Profile/page source support does not establish actual browser,
device, capture/replay or latency acceptance. Typed axes remain axes without
ordinary press judgment. Local multiplayer assignment is separate remaining
work, and polling/committed-frontier limits remain as specified above.

The portable resolved local source plan is specified in
[local input ownership](REQ__bms-local-players.md). Its owned numeric routes
retain full-width acquired sources without using native settings hosts or
browser product descriptions as identities. The Worker now accepts an optional
local plan through the shared nonblocking owner and renders paged local fields.
The browser page now calls that path with discovered exact assignments and
individual member record selection and targeted saved comparisons. Local
network competition and actual browser/device/runtime acceptance remain pending.

## Local saved comparison ownership

The page retains an explicit stable target player on each selected saved record.
For local play every target must exist in the frozen roster; for solo play local
targets must be cleared. Preserve the aggregate eight-record/64 MiB limits, read
each file once on Worker, and admit it using the target member's actual pristine
header. Each member owns its comparison state and advances from its own actual
song frontier. Reuse common SavedOpponents and retained HUD snapshots; no saved
judgment changes live scoring, captures or shared audio.

Snapshot results include every actual member in roster order with independent
opponents/error fields. Invalid member results disable only that member's HUD;
invalid roster ownership refuses the envelope. Validate selected labels and
Own/Other metadata against the frozen selections. Periodic valid counters stay
on Worker, and Window shows only comparison failures and final summaries.

Admitted comparison count reserves a fixed region before touch routing setup.
The common renderer and touch helper share the shifted/shrunken field geometry.
HUD failure keeps this region fixed and shows unavailable status, preserving
active touch coordinates. Source integration and deferred fixtures do not prove
browser execution, generated bindings or measured performance. Local network
competition remains follow-on work; touch page remapping is source-integrated
but runtime acceptance is unverified.

### Per-member peer display admission

BrowserLocalGame admits an optional peer HUD for an exact member before
activation and before that member's touch layout is configured. It reuses the
common validated peer-prefix HUD rather than changing the gameplay evaluator.
Reserve 28 pixels for its lifecycle and progress rows plus 14 pixels per saved
record, up to 140 pixels. Healthy peer rows remain at the admitted saved-space
offset even if saved comparisons become unavailable. Peer failure hides only
its own display; saved failure hides only saved rows. Failure indicators stay
within their respective reserved space and do not alter render/touch geometry.
Invalid or unadmitted updates and duplicate/late admission preserve accepted
state. No update changes another member, advances gameplay or creates network
evidence. The Page and Worker group connection and common start paths are
specified below. These APIs
and deferred fixtures do not establish browser runtime acceptance.

The local binding's progress_words reads all actual members in frozen order:
each eleven-word row has PlayerId followed by low/high pairs for the member's
own signed song time and four unsigned cumulative counters. It is a bounded
control-side snapshot, outside input and audio callbacks. It never sums players,
substitutes a shared frontier, samples a clock or advances gameplay. It remains
available for the retained final prefix after gameplay failure. The portable
whole-cohort payload codec is specified by
[local input ownership](REQ__bms-local-players.md); the scalar BKMP v6 Session
now has an explicit group mode. Worker publishes actual group prefixes through
the shared owner; Page launches its admitted local cohort. BrowserMultiplayer
exposes new_group, send_group_progress with exact
member words and poll_group_event with typed roster/progress words and an exact
BigInt sequence. The old scalar DTO and constructor remain compatible. Both
modes reuse actual readiness, clock/start and complete-write/final-ACK state.
Group event polling shares the bounded pending-event budget and remains
available for cleanup after failure. Before entering generated WASM glue, a
host must bound roster and word inputs to 64 players and 704 words; the Rust
decoder also validates exact extents, identities and scalar score semantics.
Source compilation does not prove generated binding or network execution.

The Worker network owner's open options accept a strict boolean group flag,
defaulting to false. Setting group: true requires send_group_progress and
poll_group_event capabilities; submit_group(words, finalPrefix) then uses the
same transport and read/write/clock/cleanup loop as scalar submit. Group submissions contain an
owned snapshot of at most 704 member words (11 per player, 1..64 players),
bounded before generated WASM glue copies them. Shared, resizable or detached
buffers and wrong-mode submissions are refused without admitting a pending
submission. Only the common Rust Session validates progression,
rosters, sequence and final acknowledgement. Scalar and group event drains share
one eight-event budget; synchronous callbacks may close the owner without any
later access to its freed Session. Both modes retain one pending submission and
one exact full-write/final-ACK barrier. This owner API does not itself enable
local network UI or choose remote member HUD targets.

### Actual local Worker network integration

Live local gameplay may request the same existing multiplayer configuration.
Before moving the prepared chart, validate group binding capabilities. Before
touch layout admission, reserve peer HUD space for every actual local member.
Before opening the transport, every member's bounded canonical competition
identity must agree byte-for-byte; then create one actual group Session with
the frozen local roster. Group mode reuses the common committed start and
Window/audio activation handshake, never one start or song per member.

The optional multiplayer.peerTargets is a fixed, ordinary Uint32Array of at
most 64 complete (localPlayer, remotePlayer) pairs. Local IDs must belong to the
frozen plan and occur once; remote IDs must be positive. Copy the mapping before
awaits. A received distinct bounded remote roster must contain every explicitly
selected remote ID. Without explicit targets, pair admitted local and remote
members by their frozen roster order. Surplus local members have no assigned
opponent; never borrow another member's score. Explicit targets can assign one
remote member to multiple locals. Empty explicit targets opt out of peer score
updates while retaining shared lifecycle status and whole-cohort network participation.

Group progress uses the actual game's progress_words, validated for exact
eleven-word rows in frozen local order before entering the generated binding.
Remote group prefixes must match the accepted remote roster; map their own
ten-word progress rows into each selected member's existing peer HUD. All
periodic scores and HUD updates stay on Worker. A per-member presentation fault
disables only that member's peer display, preserving saved comparisons, local
gameplay and the shared transport. Group final outcome uses multiplayer.peers
in actual local order, with player, remotePlayer (null when unassigned), status,
progress, final and error for each member. It never aliases the first member
into the scalar multiplayer.peer field. Periodic member scores are not sent to
Window.

Capture a retained actual final group word snapshot before disposing the game,
then release local ownership and await the existing shared full-write/final-ACK
drain. Snapshot failure must still release the game and report network failure.
Cancellation and late connection completion cannot access freed owners. The
real browser/device/transport acceptance remain follow-on work.

### Local group Page launch and final results

The Page permits the existing live multiplayer option with an admitted local
source plan. It freezes and forwards that plan with the normal network setup,
then uses the existing audio-preparation and committed-start activation
handshake. Invalid or missing distinct sources still refuse launch. Rendering,
judging and periodic group scores remain on Worker; Window ignores group
roster/progress notifications and does not build a periodic member HUD.

Peer display failures are correlated to an exact admitted local player and
reported at most once per member. Unknown/missing member IDs must not change
the live group status; scalar notices retain their existing behavior. Final
group results validate multiplayer.peers as a bounded exact array in frozen
local order, with positive remote IDs or null and each existing peer summary's
exact types and counter constraints. A null remote ID cannot carry a score or
final prefix. Render each member's own validated remote prefix, lifecycle and
display error once at termination. Never use the scalar peer field as a group
fallback. A malformed group summary preserves local scores/replays and reports
comparison unavailability. Stale owner/late stop receipts cannot overwrite the
current session's result.

All live Page network sessions use the group path, including one player. A
single player receives one stable actual roster ID with the Any selector and
no device-discovery or selection step. LocalRoster.snapshot accepts an explicit
includeSolo boolean, defaulting to false for existing non-network callers; when
true for a single player it returns a frozen automatic plan with the exact
(PlayerId, Any, 0, 0) row and page zero. Distinct exact sources remain mandatory
for two or more players. Automatic scope must preserve every genuinely admitted
keyboard/touch/HID/Gamepad source and the existing solo profile eligibility;
do not filter those sources against an empty exact-source set. One-player group
play exposes no page-changing control.

The Page maps untargeted saved selections internally to that sole stable member
without mutating the user's selections. Actual Worker metadata and results
still use the full cohort schema for preparation, budgets, per-member captures,
saved comparisons and peers; do not fabricate scalar aliases. A captured
one-member replay can be selected/exported/saved through the existing local
capture flow. Ordinary non-network solo and replay stay on their existing
paths. Member counts differ between peers independently of protocol mode.

Known ceiling: Legacy scalar and group Sessions remain incompatible by design.
Native application network callers still require group integration to connect
to the Page's default group path. Multi-host rooms and actual
runtime/performance acceptance are also unfinished.

## Touch-aware local page changes

The Window marks a page change pending, freezes the maximum sequence of its
previously acquired queued/inflight input prefix, and waits for genuine step
acknowledgements to cover it. Gaps from filtered acquisitions do not require
fabricated input. No new touch Down enters the queue while pending, but existing
Move/Up/Cancel retains original metadata. Cancel/stop releases the bounded waiter
and cannot revive a stale page RPC. No transport or score state is reset.

Worker invokes the actual member's set_touch_page before publishing a new page.
A visible field uses common geometry, including immutable saved-comparison space;
a hidden field disables new contact destinations while preserving all held
ownership. The receipt includes validated actual touch visibility. Invalid
geometry preserves the page and old routing; invalid visibility is a protocol
failure. Playback uses already bound recorded events, preserving the original
live spatial decision. Source/compile and deferred fixtures are not browser
execution or measured latency evidence.
