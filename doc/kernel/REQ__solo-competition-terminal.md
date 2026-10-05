# Solo competition terminal lifecycle

Solo native competition uses the same terminal delivery, cleanup and notice
drain port as the local group. An explicit pure one-shot guard claims finalization
before any diagnostic, network or presentation effect; every later finish call
returns without those effects, including after failed delivery or cleanup. The
guard is lifecycle state only, never completed-play or peer-receipt evidence.
Late observation and start waiting refuse before changing competition state or
accessing ports after finalization has been claimed.

Pure solo delivery selection receives room mode, comparison failure, readiness
and an optional actual original MemberProgress. Bilateral delivery requires an
observed prefix, healthy comparison and real readiness. Missing progress skips
delivery, never inventing a song position or score. Room mode passes the actual
member when present and an explicit empty cancellation when absent, independent
of bilateral readiness; backend room completion authority stays unchanged.
Selection borrows original member data and introduces no ownership copy or heap.

Native composition supplies the existing terminal adapter, which performs the
single actual publication copy, bounded delivery wait, join and post-cleanup
notice drain. Native failure diagnostics, retained status and peer reporting
remain outer effects. Reporting reads peer prefixes after the drain. A Skipped
delivery remains distinguishable from Accepted port operation; neither cleanup
success nor an accepted cancellation creates gameplay proof or a ranked result.
The existing finish() compatibility entry point keeps its shape, while a
presentation-injected entry point permits deterministic offline lifecycle tests.

Independent deferred fixtures cover all solo gates, missing room versus bilateral
progress, original sparse IDs/large exact values, one-shot behavior across all
terminal failures, offline presentation idempotence and late-operation refusal.
Tests use injected ports or pure offline owners, no real socket, clock, device or
renderer. Assertion execution, native/browser acceptance and formal review/QA
remain deferred. Compilation is not test execution evidence.
Five independent groups are authored and compiled only. Both writers stopped
before scoped formatting and the four compile-only configurations, each of
which exited zero. See [the evidence scope](../changes/CHANGE__solo-competition-terminal.md).

## Known ceiling

Concrete network acquisition, ghost file loading, remaining readiness/release
and ACK/room waits still require boundary work. Whole networked solo/group owners
cannot yet be constructed through entirely pure factories. Native ACKs, thread
joins, resource cleanup and performance remain unverified.
