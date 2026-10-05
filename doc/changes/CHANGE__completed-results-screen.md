# Retained native completed results presentation

The native results presentation consumes the actual owner's immutable completion
table, with each original player identity, final gauge and full-song or practice
outcome. A pure UI component validates the whole table and retains page geometry;
the desktop adapter selects it after cleanup acknowledgement and preserves the
first accepted evidence through trailing snapshots and cleanup failure. Replay
prefixes and missing completion evidence do not establish whole-song clears.

See [the screen requirement](../ui/REQ__completed-results-screen.md).
Seven independent fixture groups were authored: three pure component groups and
four actual desktop acceptance, join/draw, retention and paging groups. Desktop
fixtures obtain real completion evidence from the public StepGameplay owner and
Mixer. They were compiled, not executed. Root scoped rustfmt and whitespace
checks succeeded. The four compile-only checks succeeded for workspace/all
targets with webtransport, headless/all targets with webtransport, WASM browser,
and WASM browser-audio; existing dead-code warnings remain. Assertions, native rendering, device behavior,
browser result delivery, persistence and performance benchmarks remain deferred.
The full player goal and required ordered reviews/QA remain open.

## Known ceiling

GPU screen inspection, browser result delivery and archive persistence remain
follow-up work. This completed-outcome panel currently shows gauge and outcome;
final score statistics and opponent comparisons need a retained presentation
alongside it. No rendered visual or performance acceptance is claimed.
