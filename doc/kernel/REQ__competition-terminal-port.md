# Competition terminal delivery and cleanup port

Shared terminal orchestration uses a generic port for borrowed final progress
delivery, cleanup/join and a nonblocking post-cleanup notice drain. It imports
no concrete socket, thread, native clock, UI, diagnostics or completion proof.
Associated errors are opaque and require no Error/Display/Debug/Clone bound.
The outer owner supplies an explicit delivery intent: skip an unobserved prefix,
send validated actual members, or retain the original preparation refusal.
Empty room cancellation and an unobserved bilateral session are distinct inputs;
the shared policy cannot infer played or completed state from either.
The retained delivery outcome explicitly distinguishes Skipped from Accepted.
Accepted means that the port accepted the requested finalization operation;
it is not a typed peer receipt or gameplay completion proof. Collapsing both
cases to Ok(()) in the retained delivery field is forbidden. The outer legacy
aggregate Result may still report successful cleanup of an unobserved session.

Preparation refusal skips delivery only. Delivery refusal also cannot suppress
cleanup or the post-cleanup drain. All three outcomes remain available to the
outer owner, with original errors retained; consuming the outcome chooses the
first error in delivery/preparation, cleanup, drain order. Later failures do not
erase earlier failures or prevent attempted cleanup. A successful join or normal
EOF is never a terminal receipt, successful delivery or live gameplay completion.

The native adapter performs one ownership copy at actual final publication,
retains room controller completion authority, and joins existing endpoint owners.
It drains the whole accepted notice batch after joining, ignoring normal Closed
only at that cleanup boundary and retaining the first other original disconnect.
Room controller cleanup retains its independent receipt/cancellation semantics;
the generic adapter drain is a no-op for rooms. No new wait loop or hidden
endpoint acquisition belongs in the shared policy.

Native group finalization delegates these effects after validating and retaining
the whole real terminal prefix. Validation failures still attempt cleanup and
drain. Keeping that validated prefix before the adapter copy avoids an extra
ownership copy. Final peer reporting and presentation remain outer effects and
run after cleanup attempts. Repeated finalization is refused before any network
effect; a finished owner cannot join or send twice. Original native completion
proof remains held by the backend and is never manufactured by cleanup status.

Independent deferred fixtures cover delivery/cleanup/drain ordering, all failure
combinations, exact opaque error identity, preparation refusal, unobserved skip,
empty cancellation and unchanged original sparse member data. Tests use no real
clock, socket, thread, UI or completion object. Assertions and runtime/platform
acceptance stay deferred; compilation is not execution evidence.
Five independent groups are authored and compiled only. Both paired writers
stopped before scoped formatting and the exact four compile-only configurations;
each completed with exit zero. See
[the evidence scope](../changes/CHANGE__competition-terminal-port.md).

## Known ceiling

Solo native finalization, endpoint acquisition and remaining room waits still
require continued boundary work. Native group final reporting/presentation and
typed backend failure retention remain outer concerns. These fixtures do not
exercise the whole group owner, which still constructs concrete endpoints, or
prove the native repeated-finalization guard through an injected endpoint.
They do not
establish actual ACK delivery, successful join, resource leak freedom, platform
acceptance, performance or complete IO separation.
