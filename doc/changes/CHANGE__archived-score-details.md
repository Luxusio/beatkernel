# Preserve exact score and timing in historical results

The current increment adds a bounded version-2 historical score record and
connects actual completed common Step gameplay exports to it. Legacy version-1
decoding and scoreless encoding remain compatible. Original player IDs associate
score tables, completion and replay identities. Historical data remains separate
from live completion evidence. Stored counters and timing are drawn from cached
pure presentation rather than Window rendering or repeated per-frame formatting.

## Known ceiling

At this increment native completed-save composition still emitted scoreless
version-1 archives. [The subsequent native score increment](CHANGE__native-archived-score.md)
connects actual native finalization while preserving legacy scoreless helpers.
Saved opponent comparisons remain unarchived. Structural validation does not
authenticate editable local records.
Cold serialization, copies and decoding may allocate; byte/grade/roster bounds
limit accepted data but do not provide allocator-failure or crash-atomic proof.
Fifteen independent fixture groups are authored for later execution: seven
literal-wire, malformed-record, whole/member association and cached-presentation
groups, five exact timing boundary groups, and three actual Step/Mixer completion
groups. The presentation group is graphics-gated. Genuine judgments and Mixer
reports are used in the owner fixtures; numeric historical boundary values are
explicitly injected and do not authenticate played charts. Existing fixture
assertions remain intact. Both writers returned terminal stop reports before
scoped Rust formatting. Source authorship alone does not prove these assertions.
Four sequential compile-only checks exited zero: workspace/all-targets with
WebTransport, no-default-features WebTransport/all-targets, WASM browser/library
and WASM browser-audio/library. Existing unused-code warnings remain. Host checks
compile fixture children, including the graphics group in the workspace check;
WASM library checks compile actual shared export/presentation code without test
children. No assertions were executed.
Assertions, generated bindings, real browser/native acceptance, formal review,
required QA, verify and close remain deferred. The full player Goal remains active.
