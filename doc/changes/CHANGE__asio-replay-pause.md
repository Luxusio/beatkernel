# ASIO recorded playback pause

Recorded ASIO playback now routes pause/resume through the same replay control
step as WASAPI, ALSA and CoreAudio. Native adapters supply either an actual clock
pair or a complete assessed presentation interval. The shared owner controls the
existing mixer pause request and publishes replay state; it adds no replay
operations or format changes. GUI Watch uses the existing F9 control.

This change implements recorded playback. Live/local ASIO and network pause
remain unfinished: live input needs an explicit boundary policy and exact
logical freeze handling before that capability can be enabled.

## Shared acknowledgement

`NativePause` shares render-grid validation, recovered transition frames and
cumulative pause-gap state between point and interval observations. An interval
retains its output origin, sample rate, actual render report and complete host
bounds. Once selected, an evidence mode cannot silently change within the owner.

The first actual observation that reaches a recovered physical transition fixes
its acknowledgement window. An exact anchor uses that anchor's original bounds;
a bracketed transition retains the request's lower bound and the crossing
observation's upper bound. Fresh host time must reach the window's latest endpoint
before acknowledgement. Repeated observations and later bounds cannot replace
that deadline. Missing observations can still acknowledge an already retained
window when fresh host time reaches it.

`ReplayPause` derives frozen song time from the exact logical playback frame,
recorded start and preroll. This remains stable when the observed resume report
already contains later active frames. Song arithmetic is checked before new
pause state is published.

The common control step stages request and observation together on a bounded
state clone. Invalid clock/report evidence or song arithmetic leaves state and
capability unchanged and sends no mixer request.

## Presentation and resume

The existing bounded ASIO presentation queue continues returning physical output
points when each block's original upper host deadline matures. Manual paused
blocks have zero playback frames; active blocks carry their full playback extent.
Validation preserves cumulative silent gaps, adjacent playback continuity and
frozen playback across observed paused blocks. Coalesced valid transitions can
include unobserved blocks. Invalid reports preserve the queued evidence and its
already admitted deadlines.

After resume, replay song projection waits until presented physical points reach
the recovered resume boundary. Older queued points therefore cannot rewind the
logical visual position when the cumulative pause gap changes. Natural completion
continues using physical presentation drain.

The ASIO wrapper obtains coherent render/clock evidence and a fresh QPC sample,
then feeds the presentation queue and common pause owner. Clock midpoints do not
replace the supplied interval. The returned window remains conditional on the
caller's timer, drift and latency assessments; it establishes no acoustic accuracy.

## Verification status

Portable fixtures have been authored for actual Mixer pause/resume, interval
acknowledgement, duplicate and missing observations, coalesced reports, invalid
evidence, queue capacity and atomic errors, plus the resumed presentation boundary
and common replay control step. They are prepared for later execution.

Five locked Rust 1.98.1 compile configurations exited 0. Their logs and exit
records are under `target/ac168-{host,windows,macos,headless,wasm}.{log,exit}`:

| Configuration | Command after `cargo check` | Exit |
| --- | --- | --- |
| Host workspace | `--workspace --all-targets --locked` | 0 |
| Windows GNU | `-p beatkernel-bms-runtime --all-targets --locked --target x86_64-pc-windows-gnu` | 0 |
| macOS | `-p beatkernel-bms-runtime --all-targets --locked --target x86_64-apple-darwin` | 0 |
| Headless | `-p beatkernel-bms-runtime --all-targets --locked --no-default-features` | 0 |
| WASM graphics library | `-p beatkernel-bms-runtime --lib --locked --target wasm32-unknown-unknown --no-default-features --features graphics` | 0 |

The first four all-target checks found missing mutable producer bindings in
four new presentation fixtures. Those bindings were corrected and all four
checks repeated successfully; initial failure logs are retained with the
`-initial` suffix. The WASM library excludes those test-only changes and passed
its first check. Scoped formatting and `git diff --check` also succeeded.
Existing WASM cadence dead-code warnings and the macOS `block` dependency's
future-compatibility warning remain.

The native ASIO wrapper requires Windows, `asio-sdk`, caller-supplied SDK headers
and an MSVC-compatible C++ compiler. Standard GNU checks do not compile that
branch. Compilation, linking and real driver behavior must be distinguished.

Test execution, native/device/GUI/audio checks, independent review and QA remain
deferred under the user's instruction. The full BMS player Goal and Harness task
remain open; source implementation does not establish runtime acceptance.
