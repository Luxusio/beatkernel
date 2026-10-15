# Actual delayed Accept receipt after Commit — 2026-10-16

The native HTTP/3 room fixture now sends a middle joiner's full actual Accept
frame while retaining its exact local completion callback. Actual matching
Commit reception reaches the common client's pending-Commit branch. The normal
case releases the original receipt immediately at its actual processing time
and requires one valid schedule. The Stop case delivers the same late receipt
before and after same-URL replacement admission; every call returns Stopped and
the stopped owner stays identical. Actual transport shutdown, independent
survivor errors, all old close attempts and subsequent real replacement progress,
final ACKs, drain and close complete the observation.

No production code, public API, dependency, manifest, trust policy, Cargo target
or runtime abstraction changed. One static-only author owned the two coupled
Rust paths; root compiled and executed only after actual FINAL/QUIET.

## Scope and exact tests

The existing fixture gains only a six-line support include. After removing those
six lines, its bytes exactly match base6f3bed92; all15original room tests/helpers
are preserved. The pending setup target's two cases are unchanged. The new
support file contains the small raw-I/O adapter and exactly two direct cases
under selected_policy_rooms::accept_receipt:

- real_three_host_delayed_accept_receipt_releases_one_schedule_after_actual_commit
- real_three_host_stop_after_actual_commit_fences_late_accept_receipt_and_replacement

Both use three genuinely selected prepared two-player cohorts. Business
admission/clock/start/progress transitions remain in public RoomPlayClient;
the test adapter moves real poll_write bytes and actual validated input only.
It preserves partial offsets, each original Instant, positive-I/O capture time
and separate processing time. Its decoder counter advances after exact push
admission and resets only on successful take. Clean transfer requires no
outstanding OutboundFrame, including offset zero, no partial write, no held
receipt and zero buffered decoder bytes. The unchanged stream, session, clock
origin and already captured schedule then pass to existing Peer/RoomPlayIo.

Only the middle host's fully transmitted Accept callback is held. Its actual
Commit must match that Accept target while the callback remains outstanding.
No local schedule/publication/Leave authority is credited prematurely. Pending
peer-prefix staging remains allowed by the common client. A normal release uses
the original id/completed_ns and actual current time, without policy relaxation
or guessed sleep. WAIT10s is an observation bound, not a start-freshness promise.
Actual stale/too-close errors would remain failures.

Stop fences all relevant public mutation/delivery APIs with exact Stopped and
post-stop clone equality. Replaying a retained received Commit in those API
assertions is a duplicate observation, not a second actual wire packet. Actual
victim finish drives transport shutdown; neither survivor is locally stopped
before its own first allowed non-timeout transport error. Each retains original
error/history. Replacement uses the exact same config/URL and different genuine
selected identity with actual IDs nonzero, unique and disjoint from all old IDs.
It is admitted while old stopped state/streams still exist. After all old closes
are attempted, genuine replacement Ready/start and every directed prefix/final,
ACK, drain and close prove liveness for that observed ordering.

Public finish preserves its original returned error. Returned TimedOut is
rejected and elapsed disposal is bounded, but drain.and(closed) may mask a later
internal idle timeout after an earlier reset. No all-internal-graceful-completion
or arbitrary future callback absence is inferred.

## Actual root verification

Base:6f3bed92c2f2d22c1ef9ab510521878212d1093a.
Test/contract source commit:a872603903147eb70068b102fcf47f7e6d206348.
Room fixture SHA256:
2cec35ca9aba504489d2b23edb673812d3c65458dbb170b3d40a991c6be00b75.
New support SHA256:
85c0c037218929bcd63505fe7bd15a567adf545516be10692091534d96dc56b5.
Competition REQ SHA256:
10df1887c62892317ac1284edb1d188ccd12d6d32cf538c1f0d9992b07567be6.
Unchanged pending fixture SHA256:
45a580bbd8bfb23f29006a2cec558ecfe8a9efe63d9a882aa94e01c96a030a73.

Cached Rust1.98.1, edition2021, locked/offline, two Cargo jobs and native
headless webtransport. Relay build and both integration --no-run targets exited0.
The fresh supervisor generated one-day P-256 trusted/unrelated valid CAs and a
server leaf with SAN localhost/127.0.0.1; exact Origin and explicit CA validation
were retained. No trust bypass, system trust edit or install occurred.

Actual fresh execution: room target17passed/0failed/0ignored/0filtered in9.23s;
pending target2passed/0failed/0ignored/0filtered in0.10s. Exact per-target names
and both summaries were checked. Total19 comprises16actual relay cases,
two actual pending UDP setups and one pure admission case. Prior independent17/0
is carried only as an explicitly source-identical baseline, not new-case proof.

Relay PID1947275 on owned UDP35212 passed process/socket inode readiness,
received SIGINT, exited0 and was joined with no forced kill. Test PID1947276
exited0 and joined; compile and verification tool sessions were terminal0.
Four critical source hashes were unchanged during compilation and execution.

Root evidence: target/wf/webtransport-delayed-accept-14129/.
build-verify/invocations.json records both actual compiler commands/exits/logs.
verify/invocations.json SHA256:
c62f6cfce591638cb983b59dd16f16a2b2565d4ad9e69aab7409bd9e84422419.
verify/tests.log SHA256:
a235c8fc3ffc58531bc7f219e006f967b5211946d8f2013e58c2af6e8881d485.
Synthetic private keys stay owner-only and ignored; their contents were never
printed or committed. baseline-carry.json records the actual prior17identities.

The first root source-preservation checker removed five of the six include lines
and failed on the remaining blank line. The corrected exact-byte check passed;
no test or production source was changed and no product command was repeated.
source-preservation-check.json preserves both the checker error and correction.

The fresh supervisor independently attempts test-group and relay cleanup, so a
ProcessLookupError race in one cannot skip the other's cleanup/report. Other
cleanup errors are recorded and fail the run. Nominal readiness/counts/joins
were executed; failure/signal branches were source-reviewed, not runtime
fault-injected. Forced kill is not graceful success; SIGKILL cannot guarantee
supervisor cleanup.

## Reproduce

Use the cached target/toolchain/env.sh environment with existing host CC/AR,
RUSTUP_AUTO_INSTALL=0, CARGO_NET_OFFLINE=true, CARGO_BUILD_JOBS=2 and cleared
inherited target RUSTFLAGS as recorded. Build the current headless relay and
start a separately owned loopback serve-multiplayer with fresh strict PKI,
exact Origin https://127.0.0.1:8443 and `--group-hosts 4`.
Set only the test child's BEATKERNEL_TEST_WEBTRANSPORT_URL,
BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN, BEATKERNEL_TEST_QUIC_CA and
BEATKERNEL_TEST_QUIC_WRONG_CA to that fresh room/PKI.

```sh
cargo build -p beatkernel-bms-runtime --no-default-features \
  --features webtransport --bin beatkernel-bms-runtime --locked --offline -j2
cargo test -p beatkernel-bms-runtime --no-default-features --features webtransport \
  --test multiplayer_webtransport_room_loopback \
  --test multiplayer_webtransport_connect_cancel --locked --offline -j2 \
  -- --include-ignored --nocapture --test-threads=1
```

Inspect the retained task-owned supervisor before reuse; it creates a new phase
and refuses overwrites. Its exact names include the accept_receipt module,
room17+pending2, all zero failures/ignored/filter, source snapshots, socket
readiness and owned bounded cleanup. Ignored evidence may be absent in a new clone;
use the durable sources/contracts and fresh actual fixture rather than an old
log or a fabricated PASS.

## Gates, artifacts and remaining original scope

Independent formal code review and subsequent independent CLI QA are pending
at this checkpoint. No automatic review/QA completion or task close is claimed.

The native headless relay remains223670608bytes, SHA256
74e28d69631d77470d7d1f2c8f4f5f1c2912a9e0467949722f27cc54e91e8790.
Browser artifacts were not rebuilt. This is not current GUI/browser/physical
output/cross-OS/acoustic-sync/latency/performance or full-workspace proof.
Prefix payloads are fixture-generated protocol data, not completed gameplay.

The new cases prove a client callback delayed after a complete real Accept write
and actual Commit reception. They do not force partial Accept/Commit bytes or
the private server's outstanding Commit receipt/terminal order. Preserve
complementary portable pending/stale fixtures. Forced late setup acquisition,
arbitrary stale actor order, post-final failures and browser/device/full-player
requirements remain original obligations. WBS12.09.03 staysV, totals136/489
(27.81%)/remaining353, original91/193(47.15%), audio12/27; full Goal active.
