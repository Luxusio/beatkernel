# Injected final acknowledgement waiting

Final acknowledgement waiting is portable policy over generic notice/admission
and clock/park ports. Fixed deadlines are unsigned nanoseconds in the supplied
wait-clock domain, separate from gameplay, song and physical output clocks. The
caller supplies the original fixed deadline; retries never renew it. Clock
regression and original clock/park failures are explicit errors.

Each loop consumes the complete notice batch through its port. Cancellation
wins over any ACK, non-Closed disconnect wins over ACK, and Closed disconnect is
tolerated only with actual final acknowledgement. An ACK can finish without
sampling a clock or admitting a message. Owner closure otherwise refuses. Only
QueueFull admission is retryable; successful admission happens at most once.
Admission itself is not final delivery. A fresh post-admission clock sample
bounds the next park to the smaller of 5 ms and remaining deadline. Expired
deadlines do not cause a positive park or a renewed interval.

Opaque port/control errors retain identity with no clone, formatting or extra
trait bounds. Policy allocates no message or queue. Native adapters still own
notice draining, cancellation evidence, message copies and channel admission;
clock acquisition and thread parking live in a separate bridge. Actual scalar
and group final-delivery methods delegate to the same policy.

The legacy native full-batch last disconnect precedence remains unchanged.
Cancellation checks precede success; cleanup cannot fabricate an ACK. Queue
pressure, failure and timeout do not claim rollback of already admitted data.

Independent scripted fixtures cover precedence, admission retries, immutable
deadlines, exact large integer times, regression and errors before/after effects.
Assertions, native clock/channel/thread/socket execution and formal review/QA
remain deferred. Room final/drain waiting has its
[separate policy](REQ__room-final-wait.md). Startup/worker waits and broader
native owner separation remain follow-up work; this policy covers bilateral
final ACK waiting.

Eight independent fixture groups are authored and compiled only. Four source
configurations exited zero after both writers stopped. See
[the evidence scope](../changes/CHANGE__final-ack-wait.md).
