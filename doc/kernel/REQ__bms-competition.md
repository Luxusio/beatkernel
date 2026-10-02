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

The initial implementation supports two peers using an explicitly selected
TCP host address or join address. A networking worker owns socket I/O; bounded
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
Windows/macOS still use the existing software call commitment; their native
calibration/frame arming ports remain required. Authored fixtures compile only;
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
explicit inside joined native cleanup. Offline/ghost/local cohorts and Windows/
macOS retain previous startup until their later port. Actual physical accuracy,
drift/device/socket execution and formal acceptance remain unverified/deferred.

Calibration requires at least100ms of advancing native host/source observations;
observed slope must stay within the discipline default1000ppm. Initial calibration
is bounded by2s, then readiness/commit and actual crossing use the configured
multiplayer setup timeout. Session bracket freshness uses start-policy max age.
A zero-length finite section uses its exact physical end marker plus native
crossing, because it cannot publish a positive-playback start acknowledgement.
