# Injected room UI host

Room controller UI effects use a generic business-owned host: attachment and
cancellation observations, request retrieval, replies, control closure, room
publication/retry and results publication. The contract imports only room
presentation values and standard ownership/error types, not player globals or a
renderer implementation. Existing IO/String errors remain the cold UI contract;
this does not introduce a new opaque error protocol or per-note virtual dispatch.

NativeRoomCompetition receives the host through new_with_host, storing it as a
generic value. Every UI call during poll, cancellation, observation, startup and
finish uses that supplied host. Request/reply capacity, UI/network identity
correlation, retry without duplicate commands, cancellation and publication
error behavior remain unchanged. Display failure cannot change gameplay score,
network receipts or create successful completion.

The existing new constructor and default host remain compatible through an outer
native UI bridge. Only that bridge performs actual player thread-local UI access.
Controller production logic imports no player implementation. Native network
ownership, result building and cleanup diagnostics are further boundary work;
generic host injection alone does not establish full pure-controller separation.

Independent fixtures instantiate the actual controller with scripted room and UI
ports, without player attachment, renderer, OS clock, threads or sockets. They
cover detached request/publication gating, cancellation, command/reply correlation and contention,
publication retry/failure and cleanup/receipt separation. Actual UI/native runtime
execution, assertions and formal review/QA remain deferred. No measured zero
overhead or whole-player completion claim follows from compilation.

Six independent groups exercise the actual injected controller and are compiled
only. The four source configurations exited zero after both writers stopped.
See [the evidence scope](../changes/CHANGE__room-ui-host.md).
