# Native gauge publication

Native solo/local committed reports now update each presentation member's common
default gauge. Gauge, score, mine damage and pressed-state validation precede any
member mutation. Empty deadline reports skip gauge clones and scratch allocation;
the legacy solo snapshot mirrors one member, while a local cohort has no aggregate
gauge. Other snapshot literals initialize the added field to the same default.

The actual native replay caller, including pause boundaries, publishes the
authoritative ReplayVisual gauge without applying incremental judgments again or
inferring operation order from cumulative mine damage. The full bridge rejects
nondefault profiles, death-summary mismatches and changes to frozen failure state.
Older replay APIs observe ordinary judgments only and retain their documented
mine-aware gauge limitation. Unattached publication remains a no-op and does not
become a headless failure-control mechanism.

Four independently authored deferred fixture groups cover actual hold/tail and
audio-admission failure prefixes, independent local gauges and later-invalid-row
atomicity, captured ReplayVisual recovery and idempotent publication, and invalid
replay profile/death/failure-revival rejection. Both authors delivered actual
terminal Writes STOPPED before scoped formatting and the authorized compile-only
checks. Workspace/all-target WebTransport, headless/all-target WebTransport,
WASM browser and WASM browser-audio each completed with exit 0. Scoped rustfmt and
git diff --check succeeded. Existing unused playfield-wrapper and WASM cadence
warnings remain. Assertions were not run.

Known ceiling: this slice does not yet draw the gauge HUD, terminate playback on
gauge failure, decide completion clear/fail, configure live gauge policy or remove
the high-level mine admission guard. Browser/device execution, tests, formal
review and QA remain deferred; the full BMS player Goal remains unfinished.
