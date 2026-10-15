# Actual WebTransport room retirement and pending setup cancellation — 2026-10-16

The existing native HTTP/3 room fixture now exercises a middle joiner leaving
or shutting down its transport at complete Collecting, positive Frozen N−1Ready
and committed-before-final milestones. All six cases create a new three-host,
two-prepared-player cohort at the exact same URL with a genuinely different
selected identity while failed old owners still retain their streams. The new
cohort receives disjoint actual participant leases. In these six cases all old
closes are then attempted while the replacement is Collecting; the replacement
subsequently continues through genuine Ready/start, all-directed progress/finals,
application ACKs, drain and close.

A separate fixture exercises the public connect_room and connect_room_play
entrypoints against an owned silent UDP sink. Each observes an actual QUIC
Initial before cancellation, preserves the original Interrupted result and joins
its worker before any controller assertion. These are pending setup tests; no
TLS/HTTP3 admission or room lease has been acquired.

Production code, public APIs, dependencies, configuration and trust policy did
not change. Both test authors performed static checks only. Root owns actual
compiler and functional verification after both authors released ownership.

## Changed surface and exact cases

The six candidate paths are the existing room fixture, the new connect-cancel
fixture, competition REQ, progress inventory, WBS and this note. The existing
nine case bodies/names remain; helper extraction preserves original input-derived
roster, deadline, readiness and one-schedule checks.

Six cases in selected_policy_rooms are named
real_three_host_<phase>_<departure>_releases_room_and_replacement_survives,
where phase is collecting, partial_ready or committed and departure is leave or
disconnect. The two new pending cases are:

- real_pending_room_admission_setup_cancels_after_quic_initial_and_joins
- real_pending_room_play_setup_cancels_after_quic_initial_and_joins

Explicit Leave reaches leave_written and its retained current-thread runtime is
driven until both survivors independently observe real transport errors. The
disconnect cases request no Leave and perform actual bounded FIN/connection
shutdown; local Stop or Drop alone is not evidence. They do not simulate abrupt
packet loss. Every failed survivor preserves its first error kind/text across
subsequent mutation/delivery APIs, retains its admitted roster and historical
committed schedule, and never fabricates final ACK or drain completion.

Replacement membership is positively observed before all old owners are
closed. Every old close is attempted before inspecting results; the public
finish errors are retained and returned TimedOut is rejected. These bounded
owner-disposal attempts do not prove graceful drain or absence of every internal
timeout: an earlier drain error can mask a later idle timeout in drain.and(closed).
The replacement then proves positive liveness by further real exchanges.

## Actual root verification

Base: 7347a799447db633eedecb385e2a71409b215e3d.
Test/contract source commit: b04365a62374f15e405410bb135a469936edc6df.
Room test SHA256: e9c6d4a8fcc60222cdace2f2e414ea8e13742c5e4baccdf479bb04e5c39b79dc.
Pending test SHA256: 45a580bbd8bfb23f29006a2cec558ecfe8a9efe63d9a882aa94e01c96a030a73.
Competition REQ SHA256: 2a85bedf858b740326da373370a3c024ee5c7fcd1fe1a332b2c94119de440bd8.

Cached Rust1.98.1, locked/offline, two Cargo jobs, native headless webtransport.
Current-source relay build and both integration --no-run targets exited0.
Fresh task-local P-256 CA/server SAN localhost/127.0.0.1 plus an unrelated valid
CA were generated; tests used explicit CA verification and exact Origin
https://127.0.0.1:8443, without system trust or TLS bypass.

Actual test execution: pending target2passed/0failed/0ignored/0filtered in0.08s;
room target15passed/0failed/0ignored/0filtered in7.82s. The supervisor checked both
literal per-target case sets and summaries, not only their combined count.
These17 cases comprise14 actual relay scenarios, two actual pending UDP setup
scenarios and one pure admission case. All prior nine cases were included.
The source-identical prior independent QA9/0 is explicitly carried as baseline
in baseline-carry.json; it is not claimed as a fresh execution of the new cases.

Relay PID1789860 bound owned UDP52110, passed exact process/socket readiness,
received SIGINT, exited0 and was joined without forced kill. Test process
PID1789861 exited0 and was joined; the supervisor exited0 with all handles
terminal. Actual pending workers are explicitly joined inside their tests.
Three critical source hashes were unchanged during build and execution.

Root evidence: target/wf/webtransport-room-retirement-14128/.
Build commands, actual exits and log hashes: build-verify/invocations.json.
verify/invocations.json SHA256:
33b88f59987e69891eb5f9c4cdaaac2a6cd1b6cad06ca4b1296c2bc8a4549547.
verify/tests.log SHA256:
d4b58e0e2f5e2c4dd2cdb23a754eeb6bf97ee6234ce4d9a19150ffe4e57b6d12.
Ignored logs and synthetic private keys are local evidence and are not committed;
no key contents were printed. Supervisor signal/error cleanup branches were
statically reviewed, not fault-injected. SIGKILL cannot guarantee graceful cleanup.

## Reproduce

Use the cached target/toolchain/env.sh environment, existing host CC/AR,
RUSTUP_AUTO_INSTALL=0, CARGO_NET_OFFLINE=true and CARGO_BUILD_JOBS=2; clear inherited
target RUSTFLAGS as recorded. No install or toolchain/network fetch is required.
Build the current headless relay, run a separately owned strict loopback relay
with fresh PKI/unique room key and set the four existing test-child variables:
BEATKERNEL_TEST_WEBTRANSPORT_URL, BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN,
BEATKERNEL_TEST_QUIC_CA and BEATKERNEL_TEST_QUIC_WRONG_CA.

```sh
cargo build -p beatkernel-bms-runtime --no-default-features \
  --features webtransport --bin beatkernel-bms-runtime --locked --offline -j2
cargo test -p beatkernel-bms-runtime --no-default-features --features webtransport \
  --test multiplayer_webtransport_room_loopback \
  --test multiplayer_webtransport_connect_cancel --locked --offline -j2 \
  -- --include-ignored --nocapture --test-threads=1
```

Inspect the retained owned-relay.py before using it: it refuses to overwrite its
fresh phase directory, checks exact15+2case sets, bounds the owned test process
group and closes/joins its own relay. A forced kill is not graceful success.

## Independent gates, artifacts and remaining scope

Formal independent code review and subsequent independent CLI QA are pending at
this checkpoint. No automatic review/QA PASS or Harness close is claimed.

Native headless relay SHA256 remains
74e28d69631d77470d7d1f2c8f4f5f1c2912a9e0467949722f27cc54e91e8790
(223670608bytes), identical to the preceding multi-host child. This is not GUI
proof. Browser artifacts were not rebuilt. No full workspace/release/browser/
physical device/cross-OS/acoustic-sync/latency/performance claim is added.

BK-WBS-12.09.03 remainsV. Outstanding client Accept/server Commit writes, forced
late successful acquisition and arbitrary stale server callback ordering remain
original requirements; the integrated driver does not expose server write
receipts. Preserve portable deterministic stale-event coverage. Actual lower-level
public client/write callback coverage can be handled in a separate child rather
than replacing the full requirement with this bounded milestone matrix.
Post-final failure phases, browser interoperability and complete gameplay remain
outside this child's proved scope.

WBS stays136/489(27.81%), remaining353; original requirements91/193(47.15%);
audio12/27. The full player Goal is active and incomplete.
