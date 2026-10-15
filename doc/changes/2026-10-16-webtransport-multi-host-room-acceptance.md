# Actual three/four-host WebTransport room acceptance — 2026-10-16

The existing room protocol now has actual HTTP/3 acceptance for three and four
hosts, with two pristine prepared BMS players per host. The existing fixture
serializes admission, checks exact participant/player snapshots at every host,
observes each partial Ready bitmap before the next Ready, and verifies one local
start schedule with the requested preroll. Its shared success path compares all
directed progress/final pairs, genuine application ACKs, separate drain, inert
post-drain steps and bounded close attempts for every host. Production code,
public interfaces, dependencies and trust policy did not change.

## Intent and changed surface

This implements the actual-network evidence requested by BK-WBS-12.09.02 under
the continuing full BMS player Goal. Existing source already supports 2..64
hosts; no production defect was presumed or discovered by this work.
The five tracked paths are the existing integration fixture, competition REQ,
progress inventory, WBS and this note. No new framework or crate was added.

The new cases live in selected_policy_rooms:

- real_three_host_room_roster_readiness_start_ordered_progress_finals_and_drain
- real_four_host_room_roster_readiness_start_ordered_progress_finals_and_drain

Each uses admitted_identity for every host's actual pristine prepared two-player
cohort. Prefix payloads are fixture-generated protocol data; this is not actual
gameplay completion or sound-output evidence. The original seven test names and
their two-host/selected-policy/leave/Origin/CA/identity scenarios remain.

## Actual root verification

Base: c1d2ed6b6305cdcc51ee8f0b525ee528a6d0fce2.
Test source SHA256: e55a000c6b13848d3186356f6f20876acde4cfcf2c7f13d734e65209a563a6d3.
Cached Rust1.98.1, locked/offline, two Cargo jobs, native headless feature
webtransport. Both current-source relay build and integration --no-run build
exited0; no dependency/toolchain installation was used.

- Before fixture edits: exact seven preserved names, 7passed/0failed/0ignored,
  test duration2.41s. Relay PID1618154, owned loopback port38326, own socket
  readiness observed, SIGINT exit0 and joined.
- After fixture edits: exact nine expected names, 9passed/0failed/0ignored,
  test duration3.69s. This comprises eight real integration cases and one pure
  admission case. Relay PID1636734, owned loopback port39383, own socket readiness
  observed, SIGINT exit0 and joined.
- Three-host success checks six directed pairs at each of tick1/tick2/final3;
  four-host success checks twelve. Every receiver's exact source player payload,
  sequence/final marker and remote ACK write is required.
- Scoped Rust2021 rustfmt, diff whitespace check and WBS ID/state/dependency
  validation exited0. Full workspace/release/device/browser QA was not run for
  this test/docs-only change.

Root evidence: target/wf/webtransport-multi-host-14127/.
build-baseline/invocations.json records the two real builds.
baseline/invocations.json SHA256
fee85b8e5b8f9d77c7d0ca459c42d5a9df337b781ee1c11bb7b654a6dc874e0f;
baseline/tests.log SHA256
56d25a969419ceb8e8831fe7ee62ade7afc2eb8b1d6d25e7fb0bf79d927235df.
verify/invocations.json SHA256
f1f012bd85fb96ecbd254578ea251c0a872e95b1f54b1066667e477eb78519a6;
verify/tests.log SHA256
4ebd30a96d15a6582339be072b7df69ee0104bdb7f3bcbae6df7eefb843656a3.
Both manifests confirm unchanged fixture/competition REQ hashes during their
respective executions. Logs and fresh synthetic private keys stay in ignored
task-local storage; private key contents were not printed or committed.

## Reproduce

Use the existing cached toolchain environment in this checkout; set
RUSTUP_AUTO_INSTALL=0, CARGO_NET_OFFLINE=true and CARGO_BUILD_JOBS=2. Use the
existing host linker/CC/AR from target/toolchain/env.sh. Clear inherited target
RUSTFLAGS as done in the recorded invocations.

```sh
cargo build -p beatkernel-bms-runtime --no-default-features \
  --features webtransport --bin beatkernel-bms-runtime --locked --offline -j2

# Run a separately owned loopback relay with a fresh P-256 CA/server leaf:
# SAN DNS:localhost,IP:127.0.0.1; explicit exact Origin; no trust bypass.
target/debug/beatkernel-bms-runtime serve-multiplayer \
  --bind 127.0.0.1:<owned-port> --cert <owned-server.pem> \
  --key <owned-server.key> --origin https://127.0.0.1:8443 --group-hosts 4

# Pass these four variables only to the test child:
# BEATKERNEL_TEST_WEBTRANSPORT_URL=https://127.0.0.1:<owned-port>/rooms/<unique-key>
# BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN=https://127.0.0.1:8443
# BEATKERNEL_TEST_QUIC_CA=<fresh-trusted-ca.pem>
# BEATKERNEL_TEST_QUIC_WRONG_CA=<fresh-valid-unrelated-ca.pem>
cargo test -p beatkernel-bms-runtime --no-default-features --features webtransport \
  --test multiplayer_webtransport_room_loopback --locked --offline -j2 \
  -- --include-ignored --nocapture --test-threads=1
```

The retained owned-relay.py records exact argv, source hashes, test names/counts,
log hashes, own socket readiness and bounded process exits. It generates a
one-day P-256 CA and unrelated valid CA with basicConstraints CA:TRUE and
keyCertSign/cRLSign; a CA:FALSE leaf with serverAuth/digitalSignature and the SAN
above is signed by the trusted CA. No system trust, browser NSS or previous PKI
is reused. Inspect this supervisor before reuse; it creates a new phase
directory and refuses to overwrite it.

Ordinary SIGINT/SIGTERM enter finally, suppress repeated signals during cleanup,
terminate/join any still-live owned test group, and SIGINT/wait the owned relay.
A forced kill is a cleanup failure. These signal/error branches were statically
reviewed; they were not fault-injected at runtime. SIGKILL cannot guarantee
graceful supervisor cleanup.

## Evidence boundaries and remaining work

Noncreator request_seal refusal is the client API's authority check before any
Seal is queued; it is not a malicious-wire server-authorization test. The
creator's Seal and all snapshot/Ready/start/progress/ACK/drain observations use
the actual connection. The wrong-Origin and unrelated-valid-CA cases prove
actual refusal, not every possible internal TLS diagnostic.

Positive partial snapshots prove the server applied the preceding Ready states.
They do not prove absence of every arbitrarily delayed future frame. Each
schedule belongs to its own elapsed clock; raw cross-host target equality is
not asserted.

Known ceiling: actual room coverage is native loopback with three/four hosts and
two prepared local members per host — expand when validating 64-host limits,
multi-stage leave/rejoin, browser interoperability or full gameplay/physical
synchronization in their existing WBS tasks.

The rebuilt headless relay SHA256 is
74e28d69631d77470d7d1f2c8f4f5f1c2912a9e0467949722f27cc54e91e8790
(223670608bytes). It differs from the previous e9167621… native artifact and is
not current GUI proof. Browser artifacts were not rebuilt; this child makes no
current browser/graphics/device/performance claim. Runtime optimization evidence
from previous children remains scoped to their actual sources/artifacts.

## Independent gates and WBS

Fresh formal code review and independent QA have not run yet. Root execution
above is development evidence, not independent QA or lifecycle attestation.
BK-WBS-12.09.02 remains V until its applicable final gates are satisfied.
WBS remains136/489 complete, remaining353; audio kernel12/27. The full Goal is
active and incomplete.
