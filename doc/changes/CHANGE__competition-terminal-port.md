# Inject competition terminal delivery and cleanup

Move native group final delivery, join and post-cleanup drain behind a generic
terminal port. Shared orchestration preserves all outcomes and the first original
error while attempting cleanup after refused preparation or delivery. Validated
local terminal progress and actual backend completion authority remain separate.
Repeated group finalization refuses effects before a second send or join.

The retained delivery result distinguishes Skipped from Accepted. Accepted
records acceptance of the port operation; it cannot create a peer receipt or
live completion proof. Cleanup/drain success for an unobserved session remains
distinct from requesting final delivery, including empty room cancellation.

Native group finalization retains one validated terminal prefix before the
adapter ownership copy, avoiding an additional copy introduced by the boundary.
It preserves original boxed preparation/transport errors while copying native
diagnostics only for retained failure state. Peer reporting and presentation
still run after cleanup attempts. The adapter ignores normal Closed only during
post-cleanup drain and consumes the complete accepted notice batch.

Five independent deferred groups cover all eight Send failure combinations,
four Skip combinations, four preparation-refusal combinations, explicit empty
delivery versus skip, and successful borrowed delivery with later cleanup/drain
failure. Opaque associated errors need no error/formatting/clone trait and retain
their original identity. These pure fixtures do not exercise native repeated
finalization guards, actual delivery, join or resource cleanup.

Actual ACK/network/thread/platform behavior, assertions and formal review/QA
remain deferred. Solo finalization and endpoint acquisition remain following
boundary work; no full player completion or complete IO separation is claimed.

## Compile-only evidence

Both paired writers returned terminal Writes STOPPED after aligning the typed
delivery status and fixture API. Scoped formatting covered only the five changed
Rust paths, with whitespace checking. All four compile-only configurations
completed with exit zero: workspace/all-targets with webtransport, runtime
all-targets with defaults disabled and webtransport, wasm32 library with browser,
and wasm32 library with browser-audio. Existing dead-code warnings remain.

Five fixture groups were compiled, not executed. No assertions, apps, socket
sessions, browser/generated WASM, benchmark, formal review/security, QA,
task verification or task close ran. Actual group owner integration, ACKs,
thread/resource cleanup and platform behavior remain unverified. The full
player goal stays active and required review-before-QA close gates remain open.
