# Scalar preparation of cross-owner score transactions

Ordinary Competition observation shall prepare the local score and each changed
saved-opponent prefix using private scalar plans and borrowed recorded batches.
Copy timing/counter values, not grade maps. Only after every owner validates may
the original score maps, cursors and times be updated in place. Existing grade
storage stays retained even when a forward prefix advances or a nonempty local
report arrives. New grade insertion may still allocate its existing map entry.

Use the same internal preflight/commit mechanism for ScoreSummary.observe, local
competition reports and multi-batch ghost prefixes. Preserve public APIs, exact
opaque grade counts, timing, combo, maximum combo, and all-or-nothing typed-error
behavior. Within each batch, timing refusal precedes score refusal. Across ghost
batches, process in original order: score overflow in an earlier batch must win
over timing overflow in a later batch. Do not flatten the entire prefix into a
single timing-first validation that changes this precedence.

Per-grade overflow checks use exact cumulative matching hits when the broad
total-hit bound cannot establish safety. Unrelated saturated grades remain
accepted. Scalar plans are private to the exclusive owner transaction; expose
no public deferred commit API that can apply a stale plan after state changes.
Use static generic iteration over borrowed slices, no owned event copies,
scratch maps, heap-allocated plan objects, dynamic dispatch, locks or clocks.

Reuse the cold-sized prepared-update slots. A later failure clears staging and
preserves all scores/times/cursors for an unpoisoned retry. Equal prefix cursors
still need no score plan. Late replay admission applies the correct initial
prefix. Backward rebuild prepares from defaults before committing; resetting
old maps on a successful cold rebuild is allowed. Failure before commit retains
the old maps. Keep original recorded-frontier behavior and synthesize no misses.

## Evidence and known ceiling

Independent fixtures compare whole transactions against the former cloned
reference, cover batch-specific error precedence and cumulative grade limits,
and verify node addresses for advancing forward prefixes and nonempty reports
over known grades. Existing owner and score tests must retain their assertions.
Address checks do not constitute a global allocator audit. Cold replay admission,
new grades and backward/reset reconstruction can allocate. Near-overflow checks
may rescan earlier borrowed batches; no benchmark or global performance ranking
is established. Tests, fault injection, allocator profiling, runtime acceptance,
formal review, QA, verification and close remain deferred.
