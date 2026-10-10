# Native collector final-input delivery

The native player must deliver final acquired input and completion markers
before reporting exhausted successful collector closure. Previously the
collector could return final data with `closed=true`, and the solo/cohort
playback loop returned before processing that data. The startup handoff could
also close while transferring input retained during preparation.

The shared collector and startup bridge now keep nonempty final delivery open.
The next empty, exhausted successful acquisition closes through the existing
player path. Fatal acquisition/cleanup errors and explicit adapter aborts
retain their existing behavior. EOF grants no new clock observation or song
completion; only input covered by genuine audio/acquisition evidence can be
judged and captured.

Development reproduced three failures before the fix and passed the 18-test
collector/startup regression group afterward. All five edited Rust files pass
Rust 2021 formatting, and the current diff passes whitespace checks. Evidence
is under `target/wf/player-input-clock-fault-schedules/` and is machine-local.
The connected audio-pump development group also passed 41 tests, including five
new campaigns through the actual spawned collector, shared input handoff,
merger, audio authority, solo/cohort Runtime and capture. These check one through
four players, bounded event/cut splitting, missing/stale clock observations,
fatal acquisition/cleanup after a committed prefix and joined cancellation.
An initial 40/one failure run exposed a test oracle using a solo-only preparation
record for a cohort; the corrected oracle checks actual published player IDs
with duplicate counts preserved.

Independent DEEP code review and security review passed without findings.
Subsequent independent CLI QA on `bd8f881` passed the complete app library
(2257 passed, zero failed, six ignored), native main tests (307 passed, zero
failed, five ignored), current production builds and seven direct CLI paths.
Offline rendering produced exactly 1000 mono zero frames / 4000 bytes; missing
evdev input returned an explicit initialization error. QA also verified all five
changed Rust files' formatting, whitespace checks and the WBS inventory.

Implementation commits are `3f5477b` (collector/startup behavior) and `9fce422`
(connected regression tests). Final evidence is in
`target/wf/qa-cli-input-eof-01a12318-1/`; logs are ignored and machine-local.
Final documentation review and Harness verification/close follow these results.

## Known ceiling

These deterministic tests use synthetic input and supplied native observations.
They do not establish physical device timing or acoustic latency. Those remain
active WBS requirements. The broader WBS13.04 fault matrix remains open, and
the overall 193-item player Goal is unchanged.

See the [collector contract](../kernel/REQ__native-input-collector.md) and
[verification guide](../verification/GUIDE__runtime-fault-schedules.md).
