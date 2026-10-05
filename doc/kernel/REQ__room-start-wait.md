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
