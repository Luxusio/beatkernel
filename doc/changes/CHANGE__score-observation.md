# Retain score grade storage across committed reports

The common ScoreSummary observation method now validates scalar counters,
timing and grade arithmetic before updating existing storage. Ordinary reports
over previously seen grades no longer clone and replace the full BTreeMap.
Public score fields, opaque grades, ordered counters and atomic rejection stay
compatible; all existing live, replay, local, ghost and native callers use the
same method. Timing overflow retains its existing precedence.

Competition observation with no loaded saved opponents also uses this atomic
method directly, avoiding its outer summary clone. Successful empty reports
still publish exact song time. Backward time retains its precedence, and a failed
score batch preserves the previous score and time. This applies to an empty
opponent list even when the configured opponent capacity is positive.

The preflight uses borrowed events and scalar scratch. Only potentially
overflowing existing grade counts require an exact matching-event scan; an
unrelated exhausted grade cannot reject an otherwise valid report. New grade
insertion retains the existing BTreeMap allocation behavior.

## Known ceiling

This removes complete grade-map cloning from ordinary observations, not all
scoring allocations. Large batches with many existing near-overflow grade counts
can require repeated matching-event scans. Setup/public-state extremes remain
supported without assuming internal counter consistency. No benchmark, allocator
profile or world-leading performance claim is established by source changes.
At this increment, saved-opponent Competition updates and GhostOpponent prefix
transactions still cloned summaries and used a temporary update vector.
[The following retained-update increment](CHANGE__competition-updates.md)
addresses that vector and unchanged prefixes.
[The subsequent scalar transaction increment](CHANGE__score-transactions.md)
removes nonempty local staging and changed forward-prefix map clones.
Rebuild/loading retain their ordered prefix meaning and cold allocations.

Nine independent reference, boundary, atomicity and storage-retention fixture
groups are authored for later execution. Deterministic ordered batches and split
reports compare against the prior transactional reference; explicit expectations
cover opaque grades, custom stages, extreme deltas, exact counters, timing-error
precedence, unrelated exhausted grades and late overflow. Existing-node address
checks concern storage retention rather than a complete allocator audit. Both
writers returned terminal stop reports before scoped Rust formatting.
Two owner groups use a genuine parsed chart, judge and pristine capture header;
zero and positive configured capacity with empty opponents retain grade storage,
publish exact week/above-2^53 time, and preserve time/error precedence. They do
not exercise a loaded saved-opponent transaction or invent completion evidence.
Four sequential final compile-only checks exited zero: workspace/all-targets
with WebTransport, no-default-features WebTransport/all-targets, WASM
browser/library and WASM browser-audio/library. Earlier workspace and minimal
WebTransport checks also exited zero before the no-opponent owner extension;
the final checks include that extension and its two fixture groups. Existing
unused-code warnings remain. Host checks compile the fixture children; WASM
library checks compile the actual common method but not its test children.
Tests, benchmarks, actual browser/native acceptance, formal
review, required QA, verification and close remain deferred. Full Goal remains
active and unfinished.
