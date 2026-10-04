# Browser coordinated final room drain

BrowserRoomOwner now requires the common drain bindings and exposes one shared
drain promise. A fixed I/O deadline covers waiting for local receipts and actual
room completion; Ready is requested once. Four change-driven receipts preserve
local completion separately from drainComplete. Completed loops start no new
clock/I/O work. Close rejects pending drain, joins tracked API continuations and
late acquisition cleanup, and preserves surfaced cleanup failures separately
from the operational error.

Natural Worker completion publishes the actual final prefix before releasing
game resources, then waits for room drain and joined cleanup. One retained room
can receive bounded peer/receipt updates without touching the freed game. Stop,
Leave, replacement and handled failure cancel that old lifetime. Replacement
waits for cleanup and refuses new preparation on surfaced cleanup failure.
Final outcomes distinguish complete, cancelled and failed; network drain failure
alone preserves a valid completed local replay. Window shows only the final
event status and allows 20 seconds for natural room stop, retaining the existing
10-second timeout for other stops.

Independent deferred fixtures add three Owner groups (19 total), three Worker
groups (91 total) and one host group (94 total). They cover shared deadline and
Ready-once barriers, game release before drain, retained evidence, distinct
failure/cleanup outcomes, cancellation, stale owners, replacement cleanup and
final event status. Existing groups and mock binding contracts are aligned.

After both writers stopped, source inspection and whitespace checks completed.
No JS parser, Node, tests, browser or live transport execution occurred, and no
Cargo checks were repeated for this JS-only change. Known ceiling: tracked API
joining does not prove best-effort underlying browser/OS transport teardown;
generated WASM bindings, live TLS/browser/device/audio acceptance and multi-host
HUD/native application activation remain unverified or pending. The full player
Goal and Harness task remain active; no formal review/QA completion is claimed.
