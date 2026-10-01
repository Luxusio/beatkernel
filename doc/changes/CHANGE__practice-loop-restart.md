# Observed practice loop and usable Watch pause controls

Live nonnetwork play supports an exact start/end region: F7 marks the start,
F10 marks a later end, and F11 toggles repetition. The first observed Playing
position at or beyond the end prepares a checked retry at the original marked
start; the old native owner cancels, drains and joins before replacement. Local
cohorts retain shared ownership and individual judging. Recording attempts keep
the original base and increasing retry ordinal. Paused/pending desired pause,
pending restart, cancellation and hidden UI fence admission. Invalid end marks
preserve the region; new starts clear it. Cancel, ordinary F5 retry and failures
disable repetition. Watch and network sessions cannot repeat.

The Watch Pause button now dispatches through the same actual native-capability
guard as F9. Pause previously overlapped the later Cancel/Return hit target;
separate footer rectangles now make all controls independently selectable.

Known ceiling: coalesced UI observations may overshoot the marked end; native
teardown/reopening/loading leaves a gap. Missing observations or completion
before observed crossing cannot initiate another attempt. Sample-exact native
loop-end fencing, gapless looping, ASIO presentation, network pause policy,
browser/full widget host/native controls and full native GUI replay acceptance
remain open. Pure endpoint, bridge/control routing, pause/network/lifecycle,
single-retry/recording provenance and failure fixtures are authored/compiled
only. Tests, apps, native devices and formal review/QA remain user-deferred.
