# Offline gauge sound stop connection

Offline rendering now observes default gauge from every actual Runtime report
and uses the shared committed-fence voice Stop API on first numeric failure.
It preserves the failure operation's results and Play prefix, suppresses later
gameplay through the real Runtime fence, and continues BGM and the caller's exact
PCM frame extent. Original judge/gauge/Play/Stop admission errors are retained
together, with the latest raw render evidence and already written prefix.

Setup prepares the gameplay/BGM ownership collision check; actual failure rejects
a shared BGM namespace before stopping it. Only actual accepted owned Stops may
explain cumulative unknown_stops in this renderer's fresh queue. Generic
render_block still uses zero allowance, all other execution diagnostics remain
strict, and raw counters and public report/error fields remain unchanged.

Four independent deferred fixture groups cover fatal held/equal-time operation
prefixes, BGM/extent and block division, silent/avoided/recoverable/no-mine and
zero-frame cases, partial Play/Stop queue refusals, and real Mixer diagnostics.
Both writers delivered actual terminal Writes STOPPED before root scoped rustfmt
and all four authorized locked compile-only checks. Workspace/all-targets with
webtransport, runtime no-default/all-targets with webtransport, WASM browser
library and WASM browser-audio library all exited zero; git diff --check emitted
no diagnostics. Existing unused-code warnings remain. No assertions, runtime,
browser/device, performance, formal review or QA were executed.

Task remains open/PENDING and the whole Goal active; no PASS or completion receipt
is claimed. Native/replay Stop diagnostics/Worklet acknowledgement and output
completion, final clear/fail and high-level mine file admission remain unfinished.
