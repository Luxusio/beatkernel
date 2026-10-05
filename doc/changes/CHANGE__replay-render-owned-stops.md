# Replay render owned stop evidence

Synthetic offline and replay PCM rendering now share checked OwnedStopEvidence
and render_block_with_stops. Replay records a Stop only after its private producer
accepts the actual command; synthetic offline records only accepted Runtime Stop
reports. Play, rejected, requested and output-excluded commands earn no Stop
allowance. Raw unknown_stops remains visible; the shared checker allows only the
actual accepted count and keeps every other diagnostic strict. Generic render_block
retains zero allowance. Public reports, exact output extent, BGM, stable command
order and full legacy judge results/hash remain unchanged.

Four independent deferred groups cover actual fatal legacy replay PCM/BGM and
full-log evidence across blocks, partial queue Stop refusal and written-prefix
evidence, output cutoffs/preroll/zero/no-mine behavior, and shared evidence against
actual queues/Mixer with strict generic and other-error checks. Existing synthetic
offline fixture files remain unchanged. Both writers delivered actual terminal
Writes STOPPED before root scoped rustfmt and the exact four authorized locked
compile-only checks. Workspace/all-targets webtransport, runtime no-default
all-targets webtransport, WASM browser lib and WASM browser-audio lib exited zero;
git diff --check emitted no diagnostics. Unused-code warnings include the retained
generic render_block, now used by deferred fixtures rather than production owners,
and previously unused platform cadence/playfield code.

Assertions, application/browser/device output, performance, formal review and QA
were not executed. Task remains open/PENDING and the whole Goal active; this is
not a PASS receipt or completion claim. Native output/Worklet acknowledgement,
failed-session completion, final clear/fail and high-level mine admission remain
unfinished.
