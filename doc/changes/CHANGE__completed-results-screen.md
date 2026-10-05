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
follow-up work. No rendered visual or performance acceptance is claimed.

## Frozen score and comparison details

The first accepted table now also captures the original per-player score and
available competition prefixes. Detail cards include exact hits, misses, combo,
maximum combo and integer timing summaries; additional cards keep caller-defined
grade counts reachable. Comparison cards preserve own/other recorded prefixes,
their original recording extent, and self-reported peer prefixes with their
connection status. Up to eight supported ghosts and one peer per player remain
reachable using retained pages. The existing comparison toggle and previous/next
controls now select the completed presentation's actual mode and page count,
including solo comparisons. Trailing cleanup snapshots cannot replace the frozen
statistics or reset a valid comparison page. All packets are prepared at first
acceptance; page composition performs no formatting or gameplay processing.

This extension adds nine independent deferred fixture groups (five component,
four desktop). Component tests inspect actual emitted glyph geometry, exact
counters and prefix extents, packet reuse, clipping and all 144 pages of the
maximum 64-player comparison presentation. Desktop tests use real StepGameplay
and Mixer completion to cover freezing, mode/page controls and atomic recovery.
Root scoped formatting and whitespace checks succeeded, as did the same four
compile-only configurations listed above. No test assertions, browser/desktop
runtime, GPU, native device, allocation or performance checks were executed.
