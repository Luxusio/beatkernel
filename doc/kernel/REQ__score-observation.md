# Atomic score observation without full grade-map replacement

ScoreSummary consumes the actual ordered committed judgment batch. Preserve its
public fields, opaque grade IDs, timing statistics, counters, error variants and
all-or-nothing rejection behavior. Empty input is an exact no-op, even if public
counters are exhausted. Timing errors retain precedence over score overflow.
Custom stages still contribute score counts while existing TimingSummary policy
determines which stages contribute timing samples.

Ordinary observations over previously seen grades must retain the existing
BTreeMap storage rather than cloning/replacing the complete score summary.
Validate timing, hits, misses, combo, maximum combo and actual per-grade count
arithmetic before committing any field. All public states remain supported:
an unrelated grade at u64::MAX must not reject a batch that does not hit it;
near-limit grade arithmetic must use actual matching hits, not an overestimate.
Successful observation commits the same results as the prior ordered algorithm.
An error leaves every field and grade entry unchanged, including timing.

Use scalar scratch and borrowed event/map iteration. Add no per-report scratch
allocation, dynamic dispatch, locks, clocks, dependency or new crate. First
observation of a new grade may allocate its existing BTreeMap entry; this policy
does not promise globally allocation-free scoring. The rare near-overflow path
may scan matching events to establish exact counts before mutation. Existing
live, local, replay, ghost and native score owners use this actual common method.

When Competition has no saved opponents, its ordinary observe path must use
the atomic local score method directly and retain its grade storage. A backward
song-time request still refuses before score observation. Success publishes the
same exact song time even for an empty report, and rejection retains the prior
score/time. When saved opponents exist, keep the original cross-opponent atomic
transaction and prefix behavior. Retained scratch and unchanged-prefix storage
are governed by [the saved-opponent update policy](REQ__competition-updates.md);
nonempty local staging and changed forward prefixes use private scalar plans
under [the score transaction policy](REQ__score-transactions.md).
Rebuild and loading retain their ordered prefix semantics and cold allocation.

## Evidence and known ceiling

Independent fixtures shall compare against an ordered reference model, cover
numeric boundaries and rejection atomicity, and verify existing grade storage
remains stable across successful reports containing no new grade. Stable node
addresses are evidence of storage retention, not a complete allocator audit.
No measured throughput, allocator profile or world-leading performance is claimed.
Test execution, benchmarks, browser/native acceptance and formal review/QA remain
deferred; compile-only checks do not establish these properties at runtime.
