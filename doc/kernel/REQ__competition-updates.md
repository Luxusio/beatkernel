# Retained saved-opponent update preparation

Competition continues to publish local and all saved-opponent score/time/cursor
changes atomically. A failure in any later opponent leaves every logical owner
unchanged, and staged candidates must be released before a retry. Existing
backward-time guards, accepted-operation prefix limits, rebuild/reset semantics,
opaque IDs and exact timing/counters stay compatible. No unseen operation or
miss is synthesized beyond a recording frontier.

Prepare one reusable update slot per actually admitted opponent during cold
add_replay. Do not allocate capacity for the configured maximum in new, and
do not reserve a fresh update Vec on each report. Grow storage before accepting
a new opponent; failed admission does not add an opponent or consume logical
capacity. Empty scratch storage retains capacity across success, failure and
reset. Rebuild and later admission use the same owner safely.

If a requested prefix cursor equals the opponent's retained cursor, its score
is unchanged and must not be cloned or replaced. Publish its requested exact
song time only when the complete transaction succeeds. This includes equal-time
queries, gaps before the next recorded judgment, and queries beyond the recorded
frontier. Changed or backward prefixes still reconstruct the same ordered score.
Adding a replay after local progress starts retains its correct initial prefix.

With loaded opponents but an empty local report, retain the local score instead
of cloning it; ghosts still advance through actual recorded operations. Nonempty
local reports and changed forward ghost prefixes use private scalar plans under
[the score transaction policy](REQ__score-transactions.md) for cross-owner
atomicity while retaining existing grade maps. Add no clocks, locks, dynamic
dispatch, dependencies or new crate. Public APIs remain unchanged and all
adapters use the common owner.

## Known ceiling and evidence

Reusable slot storage removes recurring preparation-vector allocation. Unchanged
and advancing forward prefixes retain grade storage; new grades and cold
reconstruction can still allocate. This is not an allocation-free or measured
performance claim. Independent pure owner fixtures shall use genuine captured
replays, compare old transactional results, exercise failure/retry/rebuild/reset,
and verify retained scratch and unchanged-grade storage. Address/capacity checks
do not replace allocator profiling or benchmarks. Tests, runtime acceptance,
formal review, QA, verification and close remain deferred.
