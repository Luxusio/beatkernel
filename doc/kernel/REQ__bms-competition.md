# Unified BMS application and competition

The BMS application stays one crate (`samples/bms-runtime`) with internal
modules and one primary executable. It composes the core, platform and BMS
adapter crates. Existing diagnostic binaries remain available. Native play,
offline rendering, replay inspection/output, saved-record competition and
live multiplayer are modes of this application. The graphical `player` mode
uses the [desktop presentation contract](REQ__bms-player.md); terminal modes
remain available. Saved/remote opponent summaries use the graphical snapshot
bridge, retaining up to eight ghost prefixes and one peer-reported prefix per
local player. Own/other records and peer progress are labeled separately;
remote song time remains independent and implies no final ranking. Waiting,
connected, disconnected and stopped states survive cleanup. Graphical/native
execution acceptance remains deferred.

Comparison snapshots contain scalar hit/miss/combo counters rather than copied
grade maps. Ghost labels use at most 64 Unicode scalars (256 UTF-8 bytes) of a
sanitized basename. Game-owner publication is limited to once per 50ms of wall
time during ordinary reports; preparation, disconnect and cleanup bypass that
limit. It uses the existing 8ms latest-state bridge and never enters the audio
callback. Coalescing affects display only; the final snapshot retains the exact
last prefixes. Full u64 counters remain exact in the view.

## Shared transport-independent session

One session owner in the existing application protocol module composes exact
setup identity matching, preparation readiness, software clock probes, start
agreement, progress and final acknowledgements. Native QUIC delegates this
orchestration to that owner; browser adapters must use the same owner. Transport
adapters retain connection, framing, partial-write offsets, cancellation and
timeout ownership. The session acquires no clock or transport itself.

Caller times are explicit nonnegative monotonic elapsed nanoseconds. Each
immutable admitted outbound frame has a checked unique ID, with at most one
in-flight frame. Only a matching completion for that entire frame advances its
write barrier; stale, duplicate or wrong IDs reject. Partial writes and a local
write completion do not imply peer acknowledgement. Send priority remains
readiness, final ACK, pending pong, next ping, start agreement, then application
progress. The application queue is consumed only at an admitted application
slot after a committed start, retaining its existing capacity.

Session events have finite storage and explicit overflow failure. Adapters
forward completion events before attempting another read that might report
EOF. Invalid caller times or fatal protocol errors make the session unusable,
preventing continuation after a partially admitted exchange. These are software
timing and receipt contracts; physical synchronization and an actual browser
HTTP/3 WebTransport endpoint remain separate implementation and acceptance work.

## Browser multiplayer transport boundary

The browser binding delegates to the same Session and FrameDecoder, with exact
i64/u64 values exposed as JavaScript BigInt. The caller must supply the actual
canonical setup identity; transport creation alone does not derive chart/rules
compatibility. Incoming chunks are sliced to the decoder's needed prefix before
crossing the WASM boundary, preserving bounded copying and unconsumed suffixes.
Decoder failures also fence the binding's session operations.

The WebTransport adapter opens one reliable bidirectional stream over an HTTPS
HTTP/3 session. It retains only a bounded received chunk, exposes bounded read
prefixes, and keeps one reader operation and one writer operation in flight.
Detached incoming backing buffers reject immediately; their reported zero
length must not be interpreted as an ordinary empty chunk.
Outbound bytes are snapshotted before awaiting the write. Only resolved complete
local writes may be reported to the shared session; they do not imply a final
peer application ACK. Finite setup and I/O deadlines, abort and remote closure
fence late results, with idempotent cancellation and cleanup. No automatic
fallback, certificate bypass, UI-driven connection or protocol implementation
belongs in this transport component.

Callable binding and transport source remain separate from browser competition
UI and the compatible HTTP/3 service. Those require
further integration and deferred browser/network acceptance.

## Browser gameplay identity and session controller

Browser compatibility bytes derive from the actual pristine gameplay judge,
profile and resolved chart seed using the same bounded header construction and
runtime version as native competition. Querying compatibility must neither
enable recording nor mutate its accepted prefix. Capture setup uses the same
header helper, retaining its existing format. Started or fenced gameplay rejects
setup queries; bounded setup refusal leaves a pristine owner usable.

One browser controller drives the actual Rust session and byte channel. It owns
a single explicit elapsed clock epoch, an absolute setup deadline covering
connection and shared preparation, duplex read/write loops, bounded submission,
event forwarding and cancellation. It never implements a second wire protocol
or judges remote scores. One pending application submission is permitted; extra
submissions reject explicitly. Local write completion and final application ACK
remain distinct observations.
Already proven local completion and final ACK are recorded before invoking
consumer callbacks, so callback-driven cleanup cannot revoke those observations.

The controller slices reads to the decoder need and credits only matching frame
IDs after complete local writes. WASM write objects are consumed and freed
before waiting on transport. Closure fences late connection/read/write results,
rejects pending callers and releases its owned Rust state exactly once. Waiting
uses an interruptible finite tick. Actual audio preparation requests readiness;
the returned software start schedule still requires caller-owned mapping and
audio startup. Window/Worker controls, that startup integration and a compatible
HTTP/3 service remain separate unfinished work.

## Saved-record opponents

The user can select their own saved replay or another player's saved replay.
Both use the same core JudgeEngine/ReplaySession as ordinary replay. Opponents
must match the local compiled judge setup, rules, seed and profile; capture
clock-domain identifiers may differ. These noncryptographic identities are
compatibility checks, not file authentication or proof of player identity.
The opponent display consumes only recorded results through the local song
time. A truncated recording must not fabricate misses after its last operation.
Section restart rebuilds the displayed prefix. Grade counts, hit/miss counts
and combo derive from actual stage-level JudgeEvents, with no implicit grade
weighting or claim of a universal BMS ranking formula.

## Live multiplayer

### Required QUIC transport

The user selected QUIC for multiplayer on 2026-10-02. Native multiplayer uses
a common QUIC transport adapter shared by Windows, Linux and macOS. The earlier
TCP connection owner has been replaced. Do not keep an implicit TCP fallback or
copy protocol logic per OS.

Transport changes retain the versioned bounded identity, readiness, clock
probe, committed-start, progress and terminal-acknowledgement state machines.
Reliable ordered streams carry compatibility/start/result controls. Any later
datagram snapshot path needs its own explicit loss/reordering policy; replacing
a stream must not silently weaken current validation or final delivery.
Networking remains on its worker and never blocks input, judging or audio.
QUIC encryption does not authenticate reported scores or make starts acoustically
synchronized. Peer certificate/server identity trust must be explicit; disabling
certificate verification is not a default connection mode.

The user selected WebTransport for web multiplayer on 2026-10-02.
Browser multiplayer requires WebTransport over HTTP/3 with a compatible server
endpoint; browser APIs do not expose arbitrary native QUIC sockets. Share the
application protocol across native and browser adapters, while implementing the
actual WebTransport session/HTTP/3 boundary rather than assuming a raw native
QUIC ALPN peer is already interoperable. This boundary follows the
[W3C WebTransport design](https://github.com/w3c/webtransport/blob/main/explainer.md).
Keep the app one crate with internal transport modules and keep authored source
MIT. Native QUIC/runtime dependencies stay outside browser-only audio builds.
The browser adapter checks API availability and required capabilities before
connection and exposes an unsupported-browser state. Session controls and final
results retain reliable ordered delivery; datagram progress is a later optional
optimization with explicit loss/reordering handling. The WebTransport adapter
and HTTP/3 session endpoint are planned, not implemented by the native QUIC
source checkpoint.

The native source now replaces TCP with this shared QUIC adapter. TLS/trust
fixtures and eventual native/browser transport execution remain required; the
standing execution and formal QA deferral is unchanged.

## Shared multiplayer protocol components

Native QUIC and browser WebTransport shall share one application framing,
progress validation, bilateral readiness, clock probe and final-ACK state
implementation inside the BMS application crate. Shared components shall perform
no socket, platform clock, thread, transport-runtime or audio operations. Each
adapter supplies its actual clock evidence and complete-write observations.
Existing native public progress/error/event types shall refer to the same
common types. Valid native BKMP version 6 bytes and state transitions shall
remain compatible; extraction shall not bump the version or duplicate logic.

The shared decoder shall preserve partial headers/bodies and consume only the
exact admitted prefix of a transport chunk. A held full frame shall consume
zero more bytes until taken. Length shall be checked before body admission;
malformed lengths or excess internal extent shall reject explicitly, without
unchecked subtraction. The checked encoder shall reject payloads above 65,536
bytes before allocation. Framing compatibility shall retain signed song time
and exact unsigned counters, magic/version checks and reliable ordered delivery.

Readiness shall require remote receipt and actual complete local frame write.
Progress shall retain sequence, cumulative counts, combo and final-prefix
validation. A final acknowledgement shall require its exact fully written
final and any parsed peer ACK fully written; admission alone is insufficient.
Clock probes and software start messages shall retain the existing shared
checked time/order rules. Common source components do not establish a usable
WebTransport session or compatible HTTP/3 server endpoint; those remain
separate unfinished integration and deferred execution work.

The native adapter uses explicit host certificate/key (`--mp-cert`, `--mp-key`)
or joining trust anchor/server name (`--mp-ca`, `--mp-server-name`). Certificate
and key reads admit bounded regular files up to 1 MiB each; malformed or
incompatible credentials reject startup. Duplicates, role-inappropriate fields
and credentials without a host/join role reject configuration. Settings drafts
may be incomplete, but actual transport creation requires all role credentials.
There is no automatic certificate-verification bypass. TLS 1.3 and application
ALPN `beatkernel-multiplayer/6` are required; early data is disabled.

One bounded bidirectional stream carries the current framed protocol. Unsolicited
unidirectional streams and datagrams are disabled. Setup uses one absolute
deadline with cancellation checks; polling retains the same handshake future.
Quiet long charts use finite idle timeout with keepalive rather than an
unbounded dead connection. Driver polling stays on the existing network worker.
Shutdown finishes and drains the send stream within a bounded timeout before
closing the endpoint. A local write or transport receipt is not application
consumption: the existing final-prefix peer acknowledgement remains authoritative,
and no new bilateral-final requirement is silently imposed.

Deferred native loopback fixtures exercise the real public Multiplayer owners
through QUIC, including compatibility, bilateral readiness, committed start,
exact progress/final acknowledgement, incompatible setup and certificate-name
failure. These ignored tests require explicit test credential paths in
`BEATKERNEL_TEST_QUIC_CERT`, `BEATKERNEL_TEST_QUIC_KEY`, `BEATKERNEL_TEST_QUIC_CA`
and the matching `BEATKERNEL_TEST_QUIC_SERVER_NAME`. The supplied test certificate
must be currently valid for that name and chain to the supplied trust anchor.
It must not authorize the negative-test name
`beatkernel-quic-name-mismatch.invalid`.
Explicitly running without prerequisites fails rather than silently passing.
No credentials are generated or socket tests executed during this source-only
phase. Later command: `cargo test -p beatkernel-bms-runtime --test
multiplayer_quic_loopback --locked -- --ignored`. Fixture compilation alone is
not a successful TLS handshake or protocol-delivery observation.

The current native implementation supports two peers using an explicitly selected
QUIC host address or join address and the role's TLS credentials. A networking worker owns socket I/O; bounded
queues connect it to the gameplay loop. Versioned finite frames, exact setup
compatibility, sequence/progress validation and finite setup timeouts reject
malformed or incompatible peers. Disconnection is explicit. Local judgments
and audio continue to use local clocks and the existing runtime, without
waiting for network packets. Remote progress remains display data.

This is casual live progress competition. Peer scores are self-reported;
accounts, matchmaking, authoritative ranking and anti-cheat are not implemented.
Each player starts their local song independently; this increment does not
claim a shared physical playback start or bounded network synchronization.
The host address must be explicit rather than silently binding all interfaces.

## Evidence

Implementation is authorized on 2026-10-01. Portable fixtures may be authored
and compilation checked. The user's existing execution/review/QA deferral is
unchanged. Socket execution, device playback, replay/live equivalence and
thread-lifecycle regression execution remain required acceptance evidence.

## Application usage and thread ownership

The primary `beatkernel-bms-runtime` binary now dispatches `player`, `play`, `replay`,
`play-replay`, `render`, `render-replay` and `compete` within the same process.
Legacy positional offline-render arguments remain supported. Use each mode's
`--help` for its native device, buffer and timing options. `play` chooses the
current host's existing native composition and creates its input/window/run-loop
and Runtime on the named `bms-game` thread. In terminal `play` the main thread
waits for that owner; graphical `player` instead owns the winit event loop and
wgpu rendering on the main thread, consuming latest game snapshots. Terminal
diagnostics may originate from the game thread. Native audio remains on
its output worker/callback; selected multiplayer uses `bms-multiplayer`.

All three native play compositions accept repeated `--ghost-self PATH` and
`--ghost-other PATH` flags, with eight opponents maximum. Replay input caps are
64 MiB, one million operations and a 4 KiB variable header. They load and
validate saved opponents before starting audio and observe actual RuntimeReport
results and song times. A selected opponent failing compatibility aborts startup.

Select `--mp-host IP:PORT` or `--mp-join IP:PORT`; ports are nonzero, numeric
addresses are explicit, and joining an unspecified address is rejected. Optional
`--mp-timeout-ms` accepts 100..120000 (default 10000). The socket worker starts
before audio, and native output waits for compatible bilateral preparation.
After startup, local play does not wait for score packets. Progress
publishes only after bilateral readiness, at most once per 50 ms of song
time. Network errors print an explicit terminal status while local play continues.
The socket owner is retained for joined cleanup outside the input/advance path.
After native cleanup the app prints the exact final local prefix and saved
opponent hit-count differences. A connected owner attempts an ordered terminal
prefix and waits until the configured I/O stall deadline for its exact peer
acknowledgement before joining the networking worker. Queue pressure is retryable
within that deadline; disconnect, cancellation, protocol errors and timeout
remain explicit failures. This bypasses periodic publication throttling without
introducing a gameplay/audio wait. The wire protocol is version6; versions1/2/3/4/5 peers
are incompatible. Retain received peer-terminal progress separately from ordinary
progress. A receipt confirms acceptance of a self-reported terminal prefix,
including aborted sessions; it does not establish completed play or a ranking.
An already parsed peer acknowledgement must be fully written before local
delivery success; subsequent unseen peer-final delivery is not guaranteed.
See the [desktop terminal-prefix contract](REQ__bms-player.md) for lifecycle rules.

`compete --chart PATH --local-replay PATH --ghost-self PATH --ghost-other PATH
[--song-ns N]` displays actual saved result prefixes, defaulting to the local
recording's last operation. It reconstructs through an exact operation cursor,
without the synthetic timeout boundary of `seek(time)`. Saved records can also
be captured with existing `play --record-replay NEW_PATH` controls and transferred
as files; the application does not silently upload or download them.

## Known ceiling

- Two unauthenticated peers with independent local starts; section restart
  requires a fresh connection. Add a room/start protocol and authoritative
  result validation when synchronized or ranked online sessions are required.
- Exact handshake identity is at most 64 KiB, sequences are u64, queues hold
  1..1024 snapshots (application default 32), worker polling is 5 ms and pending
  frames time out after 5 seconds by default. These are software controls, not
  measured latency guarantees.
- Competition displays stage hits/misses, opaque grade counts and combo;
  weighted BMS scoring is not selected. Introduce a documented scoring policy
  when a specific score/ranking formula is required.

Source checks passed on Linux host, Windows GNU and macOS x86_64. They cover
authored fixture compilation and the unified/native call paths, not execution,
linking, real socket exchange, device playback or ASIO SDK/C++ acceptance.
## Bilateral native preparation

Selected network playback waits for both compatible peers to declare native
preparation before starting audio. Assets, immutable PCM schedule, device and
input acquisition complete before one-shot readiness admission. Version6 retains
an empty ready frame; readiness requires full local frame write and remote
ready receipt. Reject duplicate/nonempty ready frames and progress/terminal
frames before bilateral readiness. Versions1/2/3/4/5 are incompatible. The existing
setup timeout bounds startup waiting; unavailable peers abort before output
start rather than silently playing alone.

The game owner services bounded native input/messages during waiting, ignoring
pre-output-origin input without judging/capturing/retimestamping. Native loss,
removal and decode errors remain explicit. Cancellation exits through existing
cleanup. Ghost-only/offline playback acquires no new wait. Publish Waiting until
ready and Connected after both sides' readiness. The main UI and audio callback
do not wait. This preparation barrier supersedes earlier independent-before-peer
startup behavior; peers still derive local audio clocks independently. Readiness
alone does not schedule a common start or prove physical synchronization;
the software clock sampling contract below extends the barrier. Fixtures are
authored/compiled only; native/socket
execution and formal acceptance remain deferred.
## Session monotonic clock sampling

Wireversion6 retains fixed ping/pong frames after compatible bilateral readiness.
Each peer completes eight sequential four-timestamp exchanges on its socket
worker, with one outstanding ping and one pending pong. Validate exact sequence,
echo, frame length, nonnegative elapsed session times and local/remote chronology.
Use i128 differences and preserve the remote-minus-local interval[t2-t3,t1-t0].
Choose minimum corrected RTT, with newest equal-delay sample; reject impossible
negative corrected RTT rather than hide it. Emit one retained ClockEstimated
result; native readiness waiting now requires that estimate before audio start.
Existing setup/stall deadlines and cancellation apply. Version3 readiness/frame
semantics continue inside incompatible wireversion6 (old1/2/3/4/5 reject).

Timing points are software encoding/parsing observations and include scheduling
and buffering. No system/wall-clock adjustment or hardware timestamp is implied.
Checked deadline conversion retains the interval, bounds observation age, rejects
backward/future observations, overflow and an already-due possible deadline.
The model assumes constant offset during sampling; it does not prove drift or
physical synchronization. The software-start commitment below extends this
barrier with preroll-aware targets; native output-zero targeting remains to implement. Fixtures are authored/compiled only.

## Committed software start

Wireversion6 extends software clock sampling with bilateral clock-ready, host
proposal, exact join acceptance and host commit. Configurable checked nanosecond
policy bounds proposal lead, minimum remaining lead, estimate age and uncertainty.
Reject duplicates, wrong roles/order/echo, stale estimates and close deadlines.
Host publishes a local schedule after the entire commit is written; join after
receiving the commit. Native startup waiting services existing bounded input and
cancellation until the committed local target, rejecting materially late release.
This schedules a software audio-start call; hardware output-zero, differing
device latency, measured output-zero and disconnected-peer atomicity remain unproven.
Fixtures are authored and compiled only; execution/formal acceptance deferred.

Software start defaults:2000ms proposal lead,100ms minimum remaining lead,
5000ms maximum estimate age,100ms maximum interval width and25ms maximum gate
release lateness. Shared native application extraction supports
`--mp-start-lead-ms`, `--mp-start-min-lead-ms`, `--mp-clock-max-age-ms`,
`--mp-clock-max-uncertainty-ms` and `--mp-start-max-lateness-ms`. Require host or
join; reject duplicate flags, checked conversion overflow and invalid policies.
An insufficient overall setup timeout fails rather than bypassing agreement.

## Common song target across native prerolls

Wireversion6 carries actual nonnegative preroll in clock-ready. After compatible
preparation/clock sampling the host proposes a common song target with lead plus
the larger preroll. Each peer subtracts its own preroll for the software-start
target, preserving clock uncertainty and minimum earliest lead. Different sample
rates/buffers/prerolls need not match. Native solo preparation supplies its actual
preroll for Linux ALSA, Windows shared WASAPI/ASIO path and macOS CoreAudio.
Replay/judgment identity and offline/ghost playback remain independent.
Checked arithmetic rejects overflow, negative geometry and insufficient lead.
This aligns nominal software start plus preroll, not measured hardware output-zero
or device latency/drift. Execution/formal acceptance remain deferred.

## Physical-frame startup foundation

A dedicated initially held command queue permits silent native calibration while
physical output frames advance and playback/commands remain frozen. The producer
can arm one immutable physical start frame; a straddling render emits the silent
prefix then playback frame0 exactly at that frame. Missed starts reject rather
than silently clamp. Applied first playback frame is independently observable.
Existing default queue and ordinary pause behavior remain intact. Pure checked
session/host bracketing and nominal ClockPair output-frame projection preserve
intervals, check domains/overflow and enforce future frame margins.
These prerequisites now support Linux solo network startup as specified below.
CoreAudio and WASAPI now use held-device calibration/frame arming below; ASIO
still requires its bounded-interval startup port. Authored fixtures compile only;
runtime/formal acceptance remains deferred.

## Linux calibrated initial frame startup

Linux solo network playback starts ALSA behind the initial silence gate, measures
actual advancing native pairs, obtains an early committed target and projects it
with observed slope to a future physical frame beyond the render/buffer margin.
Arm once, require matching applied-frame evidence and native presentation crossing,
then anchor transport/discipline to that physical point. Checked session-to-HOST
bracketing and retained uncertainty are independent of native physical accuracy.
Explicit startup geometry lets pause/end observers validate silent prefix and
finite suffix without weakening default guards. Preserve up to4096 actual evdev
events after arm until origin known, then use original timestamps in existing
runtime input filtering. Cancellation/loss/resync/overflow/native errors remain
explicit inside joined native cleanup. Offline/ghost/local cohorts retain previous
startup; CoreAudio/WASAPI ports are specified below. Actual physical accuracy,
drift/device/socket execution and formal acceptance remain unverified/deferred.

Calibration requires at least100ms of advancing native host/source observations;
observed slope must stay within the discipline default1000ppm. Initial calibration
is bounded by2s, then readiness/commit and actual crossing use the configured
multiplayer setup timeout. Session bracket freshness uses start-policy max age.
A zero-length finite section uses its exact physical end marker plus native
crossing, because it cannot publish a positive-playback start acknowledgement.

## CoreAudio and WASAPI calibrated initial frame startup

MacOS CoreAudio and Windows WASAPI shared/exclusive solo network paths use the
held-device frame startup contract, actual native clock observations, conservative
future projection, bounded calibration/commit/crossing and original post-arm input
retention. IOHID registry/health/loss and Raw Input foreground cleanup/removal/close
remain explicit. Windows physical stream-clock zero and playback-start coordinate
are separate: converting a WASAPI position never adds the selected playback frame.
PresentationDiscipline::new_with_playback_origin validates same output domain and
playback origin at/after stream origin before observations; default new uses the
same coordinate for both. Desired song phase subtracts playback origin, while
native conversion and source identity retain original physical stream origin.
Network keysound commands stay on logical playback grid even without manual pause.
Synthetic raw-snapshot regression fixtures must preserve physical positions and
zero phase error across a delayed start. ASIO still uses the existing software
start-call commitment; its bounded presentation intervals require separate startup
projection and SDK/device evidence. Offline/ghost/local groups and wireversion6
remain unchanged. Physical accuracy, drift, actual devices/sockets and formal
acceptance stay unverified; tests are authored/compiled without execution.

## Shared native runtime ownership

Native capabilities must be maximally abstracted while preserving their actual
semantics. Multiplayer/gameplay/lifecycle/scheduling policy is shared. A single
native_start::start_committed owns the held-device startup sequence for normalized
pair/counter backends; OS code supplies NativeStartDevice operations/evidence,
not separate calibration/commit/arming/crossing workflows. NativeStartAgreement
adapts shared LiveCompetition; fake native adapters with an actual gated mixer
exercise the same owner without sockets or native hardware. Original evidence
and clock domains remain intact. ASIO interval support stays explicit. Broader
shared gameplay-pump migration remains required; this startup migration does not
claim every native entry point is already thin. See ADR__native-runtime-boundaries.md.

Native presentation metadata may legitimately name a future host coordinate.
The shared owner retains the first qualifying crossing and its original evidence,
continues acquisition/native status/BGM service under the same deadline, and
publishes ready only once actual normalized host_now reaches the interpolated
playback origin. Reject host-domain changes/regression; cancellation preserves
cleanup ownership. Do not replace that crossing with later observations.

## Shared native solo gameplay owner

A single native_gameplay::run_gameplay owns solo native input admission, actual
pause/resume acknowledgements, transport/discipline correction, judge/capture/UI/
competition reports, bounded backlog and finite/full completion policy. Native
adapters supply observation/render/host/acquisition/end/reseed operations only.
Retain original input timestamps and native provenance; loss, removal, capacity,
clock/source changes and cleanup errors remain explicit. Pause resume reseeds
the same native observation source rather than mixing a supplied pair with raw
counters. Apply configured advance_lag 0..1s through one monotonic watermark on
all platforms; Windows must use its parsed lag too. Pending input capacity 65536,
bounded native batches 256, acquisition continues during acknowledgement waits,
judgment waits until native backlog is drained at pause/resume boundaries.
Finite completion requires native boundary plus logical prefix/input drain; full
completion requires actual judge/BGM/mixer/native drain. Caller owns native stop/
join and saving captured prefix after cleanup on all exits. Local cohorts use the shared owner below;
ASIO interval startup remains pending, with no runtime acceptance claimed.

## Shared native local-cohort gameplay

All native local cohorts use one common gameplay owner over RuntimeGroup and
InputMerger. Native adapters acquire bounded original events and observations,
while the owner handles clock validation, pause acknowledgement, ordering, lag,
judgment, each player’s captured reports/score/competition and shared completion.
Acquisition continues during pause/resume acknowledgement waits. A native backlog
prevents judgment/deadline release; actual pause/resume boundaries retain the
ordered input prefix and reconcile releases before post-resume input. Resume
reseeds the same native observation source. Finite completion requires every
member’s logical prefix and an actually committed input frontier crossing the
native endpoint. Full completion requires each member’s actual completion and
drained native/merged input; absent completion metadata is not completion.
Native setup/resource cleanup and independent create-new replay saves remain
with their owners. Linux uses bounded fair sweeps across sources; native Raw
Input/HID queues retain bounded collection and exact device-loss semantics.
Physical timing, ASIO startup parity and native runtime acceptance remain unproven.

## Shared native preparation policy

Live play and replay watching resolve omitted native defaults through one shared
preparation owner. Validate original options before querying native metadata.
Explicit settings retain their exact values and validation errors. Solo live
input may choose an available keyboard automatically; assigned local players
never trigger automatic keyboard selection, and replay watching never queries
keyboards. Native adapters expose default output identity, actual output format,
keyboard candidates and explicit ASIO replay format operations only.

Query only metadata required by omitted fields. Reject unavailable defaults,
bounded-catalog violations and invalid native formats without substituting
parser-only placeholder identities. ASIO has no implicit OS default driver; an
explicit driver remains required. Output routing, clock assessments and exact
buffer requests survive projection. Metadata work belongs to the game owner,
with no device discovery or waiting on UI/audio callbacks. Compilation does not
establish native device availability or physical timing.

## Shared local member preparation and finalization

Local 2..64-player sessions share member preparation, runtime activation and
recording finalization across operating systems. Preparation preserves admitted
native device IDs and stable player IDs, validates bindings/rosters before ghost
loading, and assigns disjoint keysound voices outside the shared BGM namespace.
The same prepared chart, seed, section and rules initialize judges, captures,
record opponents and completion metadata. Group and input merger use the same
actual calibrated transport origin; no fabricated timestamp is substituted.

Native owners stop/join output and close input before common finalization.
Finalization finishes every record opponent and attempts every independent
capture save, retaining session, cleanup and save failures. Destinations match
player identity rather than a truncating positional zip. Recording publication
remains create-new. Shared composition does not erase backend capabilities or
claim native timing, file, socket or desktop acceptance. Fixtures are authored
and compiled only while execution remains deferred.

## Shared native audio construction and BGM supply

Solo and local native sessions share queue, section-relative BGM admission and
mixer construction. The prepared sample bank supplies the exact format/rate;
platform owners supply their real render capacity and supported initial start
gate. Preserve the 1024-command live reserve, explicit output origin, preroll,
lookahead and immutable finite playback endpoint, including zero. Backend buffer
negotiation and clock/calibration evidence remain native capabilities.

Startup and gameplay use one checked BGM replenishment policy: only actual
completed logical playback frames advance its cursor, absent/paused reports do
not supply commands, and each replenishment retains the 256-command budget.
Physical initial silence and pause displacement never shift this logical grid.
Late/capacity/overflow errors and admitted prefixes remain explicit. Setup runs
on the game owner; the abstraction adds no native work inside audio callbacks.
Source fixtures are authored and compiled only; physical playback, files,
network sessions, graphics and formal acceptance remain deferred.

## Interval-preserving native startup

Committed ASIO network starts must use the common held-device startup owner with
actual output-grid observations and their complete host before/after bounds.
Validate calibration consistency against the explicitly assessed rate band, then
project the committed target uncertainty through that entire band. A past average
does not tighten future instantaneous-rate bounds. Retain the whole frame range
and select its latest ceiling frame beyond the rendered frontier plus actual
buffer. The assessed rate bound is an assumption consistent with observations,
not measured drift or an acoustic accuracy guarantee.

Preserve original native evidence, sample rate, frame origin and render identity.
At the first actual gate crossing, retain a bounded host window derived from both
bracketing observations and wait for native host time to reach its upper endpoint.
The transport's nominal anchor is distinct from this retained uncertainty. Coarse
host plateaus are permitted when bounds remain consistent. Point-based backends
keep their existing projection. Configure pause/end frame grids before arming;
seed ASIO end and discipline using the original interval observation. Initial
network gating does not enable manual ASIO pause. Committed ASIO gameplay uses
the common logical render-grid scheduler, so held startup silence never shifts
keysound commands by the physical start-frame offset. Ungated ASIO retains its
existing software-frontier scheduling.

Use actual applied ASIO buffer metadata from prepared stream evidence, never a
second requested-size estimate. Reject stale, incompatible, inconsistent or
unrepresentable evidence instead of inventing point samples. Author portable
interval and real gated-mixer startup fixtures for later execution. Ordinary
source/GNU checks do not cover the SDK/MSVC driver branch; SDK compilation, device,
network and physical timing acceptance remain deferred.

## Shared solo session finalization

All native solo sessions use one common finalization policy after their output
stop/join and input close/drop attempts. Finish competition before saving the
actual captured prefix. Attempt recording publication even when gameplay or
native cleanup failed; preserve the original capture, destination and exclusive
create behavior. The save status reflects session/cleanup errors. Return the
original first error in gameplay, output cleanup, input cleanup, then save order.
Preserve native diagnostics and original error identity; common finalization
must not replace native resource-retention or cleanup semantics. Local cohorts
retain their existing all-members policy and share the same capture publisher.
Author failure-priority and actual-capture forwarding fixtures using in-memory
publication callbacks; compilation does not establish file/device acceptance.


## Portable paired-stream ownership

A future HTTP/3 adapter shall share one bounded, OS-independent room registry in
the existing application crate. Its caller supplies monotonic nonnegative
nanoseconds and owns all transport resources. Each exact bounded room key admits
at most two participants for the current bilateral BKMP session; capacity never
evicts existing occupants. Compatibility, readiness, clocks, start and scoring
remain the end-to-end Session's responsibilities, not relay judgments. This
foundation neither authenticates participants nor implements ranked results.

Every participant lease shall be nonzero, monotonic and never reused throughout
the registry lifetime. Releasing either member of a pair shall return both
closure tickets. Unknown or stale releases shall not close a newer same-key room.
Waiters have a checked finite deadline; explicit expiry returns the precise
resources to close, including equality at the deadline. A rejected admission
shall not silently discard an expired participant. Pairing removes the waiting
deadline; active play shall not expire under a waiting-room timeout. Key, time,
capacity, ID and deadline validation shall precede membership mutation.

The registry is an implementation component, not an HTTP/3 or WebTransport
server. Endpoint/TLS/stream acceptance, pairing integration and native/browser
interoperability remain unfinished. More than two network participants requires
a deliberate multi-party protocol design; existing local-player collections
remain independently extensible. Registry fixtures are authored and compiled
for later execution; runtime checks and formal acceptance remain deferred.


## HTTP/3 WebTransport relay

The existing executable shall provide an optional serve-multiplayer mode using
a real HTTP/3 WebTransport endpoint. It shall reuse the common RoomRegistry and
forward bounded valid BKMP frames between two peers with genuine transport
backpressure. Compatibility, software start, judging and final ACK remain
end-to-end peer Session responsibilities. The relay shall not authenticate scores
or silently extend the bilateral protocol to more participants.

The server shall require explicit bind address, bounded certificate/key files,
exact allowed browser origin and bounded room/setup/I/O limits. Configured origins
shall use HTTPS, or explicit HTTP loopback for the existing local browser host;
the WebTransport endpoint itself shall always use HTTPS. Requests shall
use /rooms/KEY with the common bounded ASCII key policy. Native requests without
Origin shall require explicit opt-in. No certificate bypass, automatic secret
generation or private-key printing shall occur. Concurrent sessions and setup
tasks shall be bounded; admission requires one actual bidirectional stream.

Waiting expiry and session failures shall close exact owned resource leases.
End of one relay direction shall half-close the opposite output and give the
other direction at most two seconds to finish genuine final ACK traffic. Stop
and Ctrl+C shall invalidate ownership, close all sessions and join bounded
tasks. Late setup completion shall never resurrect a stopped server or room.
Native adapters shall be shared across operating systems; browser WebTransport
shall not be represented as a raw-QUIC connection.

Author code stays MIT. The optional pinned HTTP/3 dependency shall use permissive
license options with required notices. Source implementation and compile-only
fixtures do not establish actual TLS, server, browser or native interoperability;
those checks and formal acceptance remain deferred.

## Native WebTransport relay gameplay

Native and browser multiplayer use the same BKMP session over their chosen byte
transport. Native gameplay may explicitly select the optional WebTransport relay
client with a canonical HTTPS room URL, explicit propose/join role, configured
Origin and trust anchor. Both participants connect as clients; start role is
independent of transport connection direction. Existing raw QUIC selection
remains available. Networking stays on the common worker across operating
systems, with bounded setup/cancellation/I/O/finish ownership. No verifier bypass
or ranked authority is introduced. Feature-disabled builds refuse this mode
explicitly. Physical timing and actual browser/native interoperability require
subsequent execution evidence.

An explicit native profile override that selects a multiplayer transport
replaces the previous role/credential/Origin family together. Unrelated audio,
gameplay and timing settings remain. Credentials come from the new override;
incomplete settings stay drafts until final preparation.
