# Browser live multiplayer Play integration

AC-195 continues TASK__bms-player-desktop and the active BMS player goal. This
slice connects the existing plain DOM Play host to actual BrowserMultiplayer
and BrowserMultiplayerOwner components. Default solo Play and local replay keep
the existing automatic output path. Explicit live multiplayer supplies a bounded
HTTPS endpoint and proposer/join role; the CSP permits that selected HTTPS origin.
A compatible HTTP/3 stream-pairing service remains separate unfinished work.

The Worker derives canonical identity from its actual pristine BrowserGame.
The Window finishes sample loading, AudioHost finalization and initial command
ACKs before asking the Worker to connect and request readiness. A committed
start maps from owner-relative nanoseconds through explicit performance time
origins into the original Window input clock. Converting origin and now separately
to BigInt avoids adding small durations to a large floating-point epoch first.
This follows the cross-context coordinate conversion in the
[High Resolution Time specification](https://www.w3.org/TR/hr-time-3/#examples).

The numeric committedStartProjection helper validates a fresh bracketed audio
clock, conservative 100 ms lead and no more than 100 ms combined peer/bracket
uncertainty. It chooses the first frame at or after the target and returns that
frame's projected host origin. AudioHost.arm and BrowserGame.activate receive
that same immutable projection. Root preserves actual Web Audio presentation
evidence rather than replacing it with the software target;
[Web Audio's getOutputTimestamp](https://www.w3.org/TR/webaudio/#dom-audiocontext-getoutputtimestamp)
still supplies the existing presentation pair. Browser timer coarsening, clock
projection error and physical device latency prevent an acoustic-sync guarantee.

Only actual local counters including maximum combo produce bounded progress.
Remote summaries remain explicitly self-reported in a separate readout and never
replace the local judge. Pre-start network failure ends preparation; active loss
leaves local gameplay running. Stop invalidates gameplay first, retains its score
and replay bytes, then performs a finite independent final-prefix/peer-ACK drain.
Actual local write and actual application ACK remain separate facts. Late
callbacks remain fenced from future sessions; network failures alone do not
change genuine local capture completeness.

No dependencies, new crates, React, service deployment or license changes are
introduced. Source fixtures are authored independently and remain unexecuted.
Browser/WASM glue, WebTransport server/peer interoperability, audio/device
playback, assertions, formal reviews and browser/CLI/desktop QA remain deferred.
No PASS, verified completion or task-close claim is made. After both implementation and fixture writers stopped, four compile-only commands
returned genuine terminal exit0: workspace all-targets, application headless
all-targets, WASM browser/lib and WASM browser-audio/lib, each with --locked.
The two WASM paths retained three existing platform cadence dead-code warnings.
No assertion or JavaScript parser ran. Ten fixture groups were authored: four
Worker, three Window and three numeric-helper groups. The older preview fixture
linker admits the new static import while rejecting any attempted network open;
its six groups are unchanged. Scoped whitespace checks returned exit0. These
results establish Rust source compilation only, not browser or protocol execution.
