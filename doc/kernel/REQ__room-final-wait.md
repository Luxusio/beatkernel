# Room final/drain wait policy

Room completion waiting uses generic room observation/command and wait-control
ports. Policy imports no native room, player, thread or clock implementation.
Historical final-write, final-ACK, progress-complete and drain-complete receipts
are shared value data; the existing native receipt import remains compatible.

The network-clock deadline is signed nanoseconds in the original room domain;
the control-clock deadline is unsigned nanoseconds in its separate wait domain.
Neither deadline is renewed by retries. Each loop polls, samples both clocks,
rejects negative/regressing network time or regressing control time, and checks
both fixed deadlines before accepting any terminal outcome. Late receipts remain
history and cannot become timely success. Clock domains are not interchangeable.

Queue admission and acceptance are separate evidence. Final queueing waits for
an outstanding progress command; only queue-capacity refusal is retryable. Each
final/drain command is admitted at most once. Drain admission requires observed
final acceptance. Success needs own final and drain admissions, observed final
and drain acceptance, all four real receipts, and a noncancelled error-free
terminal. An early/incomplete/error terminal refuses without further commands.
An admission or control failure does not roll back already admitted commands.

Fresh post-command control time bounds waiting to min(1 ms, remaining control
deadline), including zero after an overshoot. Clock/command errors retain their
original associated identity without formatting or extra trait bounds. Policy
allocates no member copies. The native port supplies the existing complete poll,
clock evidence and owned command copies; native sleep lives in a private bridge.

The production natural-finish path delegates to this policy. Actual stop/join,
post-join polls, presentation and final cleanup errors remain the existing native
owner's responsibility. A completed wait alone cannot fabricate gameplay proof
or erase a later cleanup error. Room startup waiting has its
[separate policy](REQ__room-start-wait.md); native UI/owner internals and worker
waits remain work.

Independent scripted fixtures cover queue/acceptance/receipt gates, fixed dual
deadlines, long times, regressions and errors before/after effects. Assertions,
actual native clocks/threads/network/browser and formal review/QA remain deferred.
Compilation does not establish end-to-end completion or physical audio sync.

Eight independent fixture groups are authored and compiled only. Four source
configurations exited zero after both writers stopped. See
[the evidence scope](../changes/CHANGE__room-final-wait.md).

## Resumable final waiting

`RoomFinalWaitState` keeps the same fixed deadlines, previous clock observations
and own command admissions across finite `step` calls. A step performs one
policy iteration and returns `RoomFinalStep::Wait(ns)` or `Completed`; it never
parks, loops, acquires a resource or creates a timer. The caller schedules the
next step. The existing blocking wait delegates to this state and alone parks
for the returned delay. Observations, clock checks, command effects and their
error precedence remain in their original order.

Success is sticky and subsequent steps return Completed without port effects.
Any error seals the state: its original associated error is returned once and
subsequent steps refuse with InvalidTerminal without effects. Accepted commands
are never rolled back or repeated after a terminal error. A scheduling/park
failure is the caller's responsibility and must stop that wait attempt.
This component prepares asynchronous orchestration; actual browser integration
and runtime acceptance remain unfinished and deferred.

Seven additional independent step fixture groups are authored and compiled only.
All four source configurations exited zero after both writers stopped; see
[the incremental evidence](../changes/CHANGE__room-final-step.md). This preserves
the deferred execution and unfinished full-player acceptance boundary above.
