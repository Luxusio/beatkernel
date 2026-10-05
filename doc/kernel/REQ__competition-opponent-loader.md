# Injected saved-opponent loading

Saved competition opponent preparation uses a generic loader with an opaque
borrowed resource key and opaque associated error. The loader supplies an owned
decoded ReplayFile and display label under the original ReplayCodecLimits.
Shared policy imports no path, filesystem, socket, renderer, clock or concrete
native loader. Memory/cache/file adapters can provide recordings through the
same contract without additional crates or dynamic dispatch.

The complete requested count must fit the remaining configured opponent capacity
before the first loader call. Capacity failure leaves existing opponents and
local progress unchanged and performs no resource IO. Empty batches touch no
loader. Each request preserves order and original Own/Other classification;
labels come from the adapter, never from formatting an opaque key in business.

Loading does not trust the supplied decoded object as validated. Every file
passes existing Competition::add_replay whole-file canonical reconstruction,
actual chart/rules/profile/seed/runtime compatibility and recorded-operation-only
projection. No future miss/end advance is invented for a truncated recording.
Loading into an active comparison uses its existing observed prefix while
preserving the actual local score/time. Original capture clock differences remain
compatible according to the existing competition header policy.

Loader refusal returns the exact original associated error without extra bounds,
clone or wrapping. Competition refusal retains the original typed policy error.
Processing stops at the first failed request; earlier admitted opponents remain
as the explicit accepted prefix, and later resources are not touched. This is
not an atomic whole-batch transaction; fresh native preparation drops its local
comparison on failure. Admission of the failing file itself remains atomic.

The native outer adapter opens original Path values and delegates bounded replay
reading/decoding to existing read_replay. Display conversion stays in that
adapter. CompetitionOptions::load_opponents retains its signature but delegates
to shared policy; its request list is cold bounded preparation data. Capacity
preflight precedes native opens and request allocation. Loading/reconstruction
allocations belong to setup, outside audio callbacks and per-note observation.

Independent deferred fixtures inject in-memory recordings and opaque failures,
cover whole-count preflight, ordering, labels/ownership categories, original
errors, invalid decoded files and accepted prefixes, active local state and
truncated recordings. Assertions and real filesystem/platform/benchmark/formal
review/QA acceptance remain deferred; compilation is not executed test evidence.
Five independent groups are authored and compiled only. Both writers stopped
before scoped formatting and the four compile-only configurations, each exiting
zero. See [the evidence scope](../changes/CHANGE__competition-opponent-loader.md).

## Known ceiling

Endpoint acquisition and credential loading, actual file reader/OS behavior,
remaining room/ACK waits and whole networked-owner construction still require
continued boundary work and acceptance. Allocator fault injection, full pipeline
runtime evidence and complete IO separation are not established by these fixtures.
