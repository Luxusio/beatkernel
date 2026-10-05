# Injectable competition start waiting

Native solo/cohort setup waiting previously read Instant and slept directly in
separate loops. Both actual paths now delegate to one business start gate over
readiness/polling and opaque control-clock ports. Native compatibility wrappers
select system/network adapters; explicit with-ports methods also inject the
competition presentation host. Control deadlines and original network release
time remain separate; commit-only mode never reads the release clock. No new
crate or dependency was introduced.

The gate checks deadline arithmetic and control chronology without panic and
preserves readiness-once, service/poll/timeout ordering, future commitment,
release/lateness bounds and 5ms/remaining-duration waits. A second control read
at the deadline preserves the original zero wait then service/poll/timeout order.
Native owner stop/status handling and forced-publication error precedence remain
outside policy; success here never establishes play/output completion.

Eight independent deferred fixture groups cover deterministic traces, original
errors, cancellation, deadline equality, bounded waits, control faults and
commit/release limits. Actual native owner cleanup/status transitions and real
network/device behavior are outside these memory-only fixtures. After both writers stopped, scoped rustfmt and git diff --check completed.
All four root compile-only checks exited zero: workspace/all-targets with
webtransport; runtime/all-targets without defaults with webtransport; WASM
browser library; WASM browser-audio library. Existing dead-code and ClockPair
import warnings remain. Assertions, formal review/QA, device/interoperability
tests and benchmarks remain unexecuted; compilation is not assertion success.

## Known ceiling

그룹 네트워크 게시 주기와 room 어댑터 내부 대기는 여전히 네이티브 시간에 연결됨 — 후속 IO 경계 분리에서 처리.

Network ownership, storage and terminal diagnostics still require separation.
This increment does not establish full player completion, full layer separation,
SQLite-equivalent reliability or comparative performance.
