# Exact live pause and Windows ASIO integration

Offline solo and local cohort gameplay now consume typed native pause evidence
through the same common owners. Windows ASIO supplies complete assessed host
intervals; WASAPI, ALSA and CoreAudio retain their point observations. The shared
`live_pause` component uses the existing `NativePause` coordinator and sole mixer
command producer. There is no separate Windows pause state machine or SDK driver
change. Network pause remains unsupported.

The durable contract is the
[exact live pause policy](../kernel/REQ__bms-player.md#exact-live-pause-with-interval-evidence).

## Exact song position and preserved history

Pause requests, observation validation and checked boundary-song arithmetic are
staged before publishing pause state or issuing a mixer request. The frozen song
is the original song origin plus the once-rounded logical playback-frame duration.
It does not come from a presentation midpoint or a later render cursor.

A staged Transport pause and seek anchors that exact song at the selected input
cutoff while preserving historical mappings before it. Resume keeps the frozen
song and rebuilds presentation discipline with the cumulative manual pause gap.
Finite-marker precedence, startup displacement and existing cleanup remain in
the common path.

Committed judge history cannot rewind. A committed member prefix beyond the
exact frozen song, a queued pre-cutoff input mapping beyond it, or a cutoff that
conflicts with committed Transport chronology produces an explicit error through
existing cleanup. Already committed reports and captures remain intact. Timing
intervals cannot repair those history conflicts.

## Input policy and local cohorts

The original complete presentation window remains available. Acknowledgement
still waits until fresh host time reaches its latest endpoint. For software input
classification, pause uses the earliest endpoint and resume uses the latest.
Point observations use their existing cutoff as both endpoints. These cutoffs do
not claim an exact acoustic timestamp.

Physical input acquisition continues while pause is pending or active. Inputs
strictly before the pause cutoff follow their original runtime path; inputs at
or after it update paused key levels without judging. Inputs before the resume
cutoff also remain paused levels. Reconciliation releases retain source and
device ownership and precede equal-time or later original inputs. New paused
presses remain suppressed until release.

Local cohorts share one Transport and boundary. Every member's committed prefix
must be valid before pause; the owner waits for the configured input merger lag
before committing the pause boundary. Resume requires every member to be at the
exact frozen song. Per-device input ownership, fair merger ordering and member
captures remain in the existing common owner.

## Windows native evidence

One Windows gameplay adapter serves solo and local play. It retains the original
native evidence separately from the correction discipline's latest clock pair.
An ASIO poll supplies a coherent render report, output origin, sample rate and
complete assessed interval together with fresh QPC. Missing observations can
advance an already retained acknowledgement using fresh host time, but cannot
manufacture evidence or establish capability.

Resume seeds the new ASIO presentation discipline directly from the retained
original native observation. This also handles a midpoint plateau where the old
discipline was waiting for host progress and its correction pair was older than
the observation. It does not require equality with that stale pair. WASAPI keeps
its actual snapshot and exact equality check.

Offline ASIO solo/local playback now enables manual pause and logical playback
scheduling. Silent physical pause frames therefore do not move keysound command
scheduling. Native interval bounds still depend on the supplied timer, drift and
latency assessments; this source change establishes no physical accuracy.

## Verification status

Portable fixtures are authored for the shared helper, actual Mixer/Transport and
gameplay paths, exact song anchoring, input classification and reconciliation,
prefix/chronology conflicts, cohort lag and member freeze, and Windows evidence
conversion and midpoint-plateau resume seeding. They have not been executed.

All five locked Rust 1.98.1 source configurations exited 0 on their first check.
Logs and exit records are retained under
`target/ac171-{host,windows,macos,headless,wasm}.{log,exit}`:

| Configuration | Scope | Status |
| --- | --- | --- |
| Host | Workspace, all targets | Exit 0 |
| Windows GNU | BMS runtime, all targets | Exit 0 |
| macOS | BMS runtime, all targets | Exit 0 |
| Headless | BMS runtime, all targets, no default features | Exit 0 |
| WASM | BMS runtime library, graphics without default features | Exit 0 |

Scoped formatting and `git diff --check` succeeded. Existing WASM cadence
dead-code warnings and the macOS `block` future-compatibility warning remain.
The fixtures were compiled, not executed.

The native ASIO wrapper requires Windows, `asio-sdk`, caller-supplied SDK headers
and an MSVC-compatible C++ compiler. Ordinary GNU checks do not compile that
branch. SDK/MSVC compilation, linking and real driver behavior remain unverified.

Tests, native/device/GUI/audio checks, performance measurements, independent
review and QA execution remain deferred under the user's instruction. The full
BMS player Goal and Harness task remain open; fixtures and source compilation
do not establish runtime or formal acceptance.
