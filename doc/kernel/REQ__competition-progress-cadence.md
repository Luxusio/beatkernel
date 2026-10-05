# Injected competition progress cadence

Network progress publication uses a generic clock port and a portable cadence
state, independent of presentation, judge/input/song clocks, sockets and native
time acquisition. The clock supplies exact monotonic u64 nanoseconds with an
opaque associated error. One cadence state and its clock must share a stable
origin for the lifetime of a competition observation owner; switching clock
origins while retaining state is not supported.

Readiness/allowed/required-start gates suppress clock reads and publication.
An admitted publication is due initially or at least 50,000,000 ns after the
previous successful publication completed. Every successful clock read is
checked against the previous observed value, including suppressed-due attempts.
Regression is an explicit policy error. Comparison uses checked ordering and
subtraction without float conversion, saturating arithmetic or deadline addition.

Only actual successful publication followed by a valid post-effect clock read
advances the publication marker. Refusal retains the original publication error
and skips the post-effect read. Clock errors retain the original clock error.
Failure after a successful effect cannot roll back that effect; the publication
marker remains unchanged and the caller must disable comparison before retry.
Observed time and committed publication time are separate state, with no heap,
lock, dynamic dispatch, OS or render dependency in the shared policy.

Native group observation injects this clock through an explicit ports method.
Its default composition supplies a per-owner native monotonic clock outside the
shared policy. Whole local progress is validated and retained before optional
clock/network effects. A cadence or transport failure disables comparison and
requests stop without rejecting already committed local judge progress. Room
observation retains its existing independent controller cadence and bypasses
bilateral clock gates. Solo publication remains based on original signed song
time and is not switched to the group control clock.

Independent fixtures cover exact boundaries and long times, all effect
gates, read ordering, refusal, pre/post clock errors and regression, marker
semantics and opaque error identity without native clock/network execution.
Execution of assertions, runtime acceptance and formal review/QA stays deferred.
Seven groups are authored and compiled only. Scoped formatting, whitespace
checking and all four compile-only configurations completed after both writers
stopped; each compile check exited zero. See
[the evidence scope](../changes/CHANGE__competition-progress-cadence.md).

## Known ceiling

Native endpoint acquisition, start/cleanup ownership and retained prefix
allocations remain outside this change. The native clock adapter still performs
real clock acquisition when selected. No complete IO separation, allocation-free
networking, measured latency, platform acceptance or SQLite-grade reliability is
established by authoring or compiling these fixtures.
