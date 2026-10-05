# Room startup wait policy

Room startup waiting uses generic initial/poll observation and wait-control ports
and a caller-supplied service callback. Policy imports no native room/player,
renderer, clock or thread implementation. The existing native startup trait is
a compatibility adapter to this policy, with its callback/error identity retained.

Initial cancellation returns false before closure refusal; closing startup
refuses before service, poll or waiting. Each iteration runs service first.
Service cancellation/error stops before polling. A poll exposes cancellation and
its original failure together: cancellation wins over that failure, while an
uncancelled failure refuses before inspecting Leave, terminal or Commit.

Leave pending and terminal outcomes refuse before using a retained committed
schedule. Thus a late failed Commit or a closing room cannot activate output.
A valid observed Commit returns true without waiting. Pending state requests
exactly 1 ms through the injected wait port before the next service/poll cycle.
Opaque service, poll and wait errors have no added trait bounds or formatting.

This policy introduces no replacement setup timer. The original network owner
retains its setup deadline, and the caller retains cancellation/service authority.
Tests use finite scripted observations; the supplied ports must provide progress,
termination or cancellation for a wait to finish. Software Commit observation is
not physical output synchronization or proof that gameplay has begun.

Native initial/poll adapters retain actual UI cancellation and room observations.
Native sleeping lives in the existing private room wait bridge. Existing result
handling still requests stop on cancellation and preserves network diagnostics
on failure; it does not fabricate a committed schedule. Native UI/owner internals
and worker waits remain unfinished boundary work.

Independent fixtures cover precedence, zero-effect terminal paths, service/poll
ordering, repeated pending state and original error identity. Assertions, actual
UI/threads/network/platform execution and formal review/QA remain deferred.

Six independent fixture groups are authored and compiled only. Four source
configurations exited zero after both writers stopped. See
[the evidence scope](../changes/CHANGE__room-start-wait.md).

## Resumable startup wait

RoomStartWaitState owns one initial observation and a sealed terminal result.
Its finite step returns Pending(1 ms), Ready or Cancelled without waiting,
looping, timers or resource acquisition. Initial cancellation precedes closing;
after initial admission, service precedes each poll. Service false cancels;
observed cancellation precedes port failure, leaving, terminal and commitment,
in that order. Original service and port errors move out unchanged with no
Clone/Display/Error bounds. Failed states subsequently refuse with Terminal
without service/port effects; Ready/Cancelled repeat their value without effects.
The state has no Clone/Copy/reset APIs that duplicate or renew its authority.

The existing await_room_start uses this state and only its wrapper invokes the
wait-control port for pending delays. First-result behavior and control refusal
remain compatible. Service/wait acquisition and all actual host/native effects
remain adapters. Browser integration of this startup wait state is subsequent
work; the browser's protocol start agreement is already common Rust logic.
Pure scripted fixtures are authored for later execution, not startup acceptance.

Six additional finite-state fixture groups are authored, unexecuted. Four
compile-only configurations exited zero after both writers stopped; see
[the incremental evidence](../changes/CHANGE__room-start-step.md).
