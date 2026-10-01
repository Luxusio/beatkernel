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
before audio, and local play proceeds without waiting for peer setup. Progress
publishes only after a compatible handshake, at most once per 50 ms of song
time. Network errors print an explicit terminal status while local play continues.
The socket owner is retained for joined cleanup outside the input/advance path.
After native cleanup the app prints the exact final local prefix and saved
opponent hit-count differences, then joins the networking worker. A remote
snapshot remains the last received prefix with its own song time; it is not
promoted to an acknowledged complete result or a final online ranking.

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
