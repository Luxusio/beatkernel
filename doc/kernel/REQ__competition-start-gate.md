# Injectable competition start gate

The actual native solo/cohort preparation loops share a business-owned start
waiting policy with explicit network and control ports. Business code must not
read Instant, sleep, instantiate sockets, use player UI or invent host/input
clock evidence. Native compatibility methods select the real adapters; explicit
with-ports entry points use the same actual algorithm with injected controls,
network observations and presentation host. No extra crate is required.

A network start port performs readiness admission, bounded polling, supplies an
actual accepted StartSchedule, and exposes the original network release-clock
read and maximum lateness policy. A setup-control port extends existing pump
control with remaining-duration calculation, using ordered opaque moments.
Control time only establishes a finite setup deadline; network release time is
separate and cannot be replaced by control time. Commit-only mode never reads
the release clock. No successful setup establishes song/output completion.

Preserve operation order: construct deadline, admit readiness once, service
native acquisition/cancellation, poll and propagate disconnection, check exclusive
timeout, inspect commitment, optionally check original release clock, then wait
at most 5ms and at most remaining deadline duration. If the second control
read reaches the deadline, retain the original zero-duration wait followed by
the next service/poll/timeout sequence; an early return here would change
cancellation/disconnection precedence. Cancellation stops before
polling and returns false; errors preserve their original values. Poll failures
precede a same-iteration timeout. Zero/overflowing deadlines and control clock
regressions must be explicit without panic; failures remain cleanup obligations.
Future release stays pending; exact target and lateness bound are inclusive.
Negative release timestamps or exceeding lateness produce existing protocol errors.

Native owner status/stop behavior remains Waiting/Connected/Stopped/Disconnected
as before. Preserve observed failures and original setup/acquisition errors when
forced status publication also fails. Successful/cancelled setup still propagates
publication errors as before. Solo without a network returns true without reads
or waits. Cohort retains original finished/failure guards and polling semantics.
No hidden system control inside fully injected policy entry points.

Independent deferred fixtures use memory network receipts and virtual controls
for deterministic traces, commit/release distinction, service cancellation,
poll errors, deadline equality, bounded waits, overflow/regression, clock/readiness/
wait failures and release limits. Existing fixture assertions remain unchanged.
Assertions, formal review/QA, interoperability and hardware checks remain deferred.
Native competition network ownership, storage and terminal diagnostics remain
separate unfinished boundaries; this increment does not make all owners pure.
