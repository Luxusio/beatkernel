# Shared scheduled stop and replay failure audio

Runtime and the replay planner now share GameplaySoundStop: sorted unique voices,
successful-admission watermark, one-attempt latch, original callback errors and
explicit restoration. Runtime retains its committed-fence guard and actual queue
telemetry. Planning records inclusion only, not actual queue or output acceptance.

Every mine-aware replay observes actual operations with the default gauge, even
without audible WAV00. The failure operation retains its Play prefix followed by
one planned gameplay Stop sequence. Later gameplay sounds are suppressed while
complete legacy judge results/hash and BGM are preserved. BGM voice collisions
are rejected at actual failure. Empty silent failure needs no output mapping;
finite exclusive endpoints and existing no-mine command ordering remain intact.

Two independent core and three replay fixture groups were authored; the existing
five mine-audio groups are retained with test-only shared helper visibility and
the every-mine operation-order expectation aligned. Both writers delivered actual
terminal Writes STOPPED before root formatting or compilation. Scoped rustfmt
and git diff --check completed without diagnostics. All four authorized locked
compile-only checks exited zero: workspace/all-targets with webtransport, runtime
no-default/all-targets with webtransport, WASM browser library, and WASM
browser-audio library. Existing unused-code warnings remain. Assertions, browser,
device, performance, formal review and QA were not executed; this is not a PASS
receipt and the task/whole Goal remain open.

Actual owned Stop diagnostics/Worklet acknowledgement,
offline gauge connection, physical output completion and clear/fail remain
unfinished. Strict render validators and high-level mine admission stay guarded.
