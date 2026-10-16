# Actual server Commit and stale event order — 2026-10-16

The actual group serve actor now has test-only observation/hold points and
three real HTTP/3 cases. They move the original complete Commit Written value
and original terminal/Resource/permit; they do not manufacture events or clone
the production actor. The normal case applies the original receipt and requires
commitment/relay activation and later real exchange. Cancellation cases admit a
different selected cohort at the exact same URL and positively process both old
events in opposite orders before replacement Ready/start/exchange.

The added actor calls and module are cfg(test)-only. Shipped control flow,
public API, dependencies, CLI, trust validation and write_frames stay unchanged.
One static-only worker owned the actor file; a separate static-only test author
owned the new module. Root compiled only after both actual FINAL/QUIET.

## Scope and exact assertions

Each real case prepares three genuine selected two-player cohorts, checks actual
ordered IDs/roster, Seal and positive partial Ready, and captures one schedule
in each original local clock/preroll domain. Only the middle host's original
complete StartCommit Written is held. Both other server Commit receipts must
be applied with their in_flight cleared while the target is held, server
commitment false and relay inactive. Actual matching client Commits are an
additional prerequisite. The barrier is checked at hold and after later commands.

Normal replay moves the same Command once at real current time, without
retimestamping or policy relaxation. A post-processing observation must show
committed/active before all directed progress/final ACK/drain/close.

Leave and disconnect act through another real host. Survivors retain their
first non-timeout transport failure and public history before local cleanup.
The target's original terminal tuple remains in a one-shot future in the same
JoinSet, preserving its Resource/session permit and result fingerprint.
Replacement admission uses the same URL/key, different genuine identity and
nonzero unique new IDs disjoint from every old lease. An independent
after-admission snapshot proves its Collecting identity, roster/deadline,
resources/frames and absent PreparedRoom before either stale event is released.
Written then terminal for Leave, and terminal then Written for disconnect,
each receive a positive acknowledgment after the original guard/drop. Tokens,
fingerprints, increasing processing ordinals and unchanged baseline must match.
Only after both acknowledgments and all old close attempts does replacement
Ready/start, all directed progress/final ACK/drain/close run.

Notice loops service every live client and retained failed/departed runtime.
FixtureGuard releases gates, requests shutdown, attempts every client close,
observes completion after server runtime disposal and joins the actual thread.
The outer owned test process is the hard hang boundary. Public finish errors
are preserved and returned timeouts rejected; drain.and(closed) can mask an
internal idle timeout, so this is no all-internal-graceful-completion claim.

Four new tests live under multiplayer_webtransport::group::network_fixtures:

- unscoped_event_hooks_preserve_commands_and_shutdown
- real_server_original_commit_receipt_activates_exact_room_after_actual_commit_write
- real_server_leave_replacement_survives_written_then_terminal
- real_server_disconnect_replacement_survives_terminal_then_written

The first is an ordinary unit test; the three real cases are ignored by default
and were explicitly included with fresh PKI. All13 old group cases and all19
existing integration cases, including their four source files, are unchanged.

## Actual root verification

Base: 6a771beeaa6c49530c510fd2c6cbac3c136ed23c.
Cached Rust1.98.1, edition2021, offline/locked/two Cargo jobs, existing host CC/AR.
Current headless relay build and library plus both integration --no-run passed.
The first no-run failed E0061 because exercise lacked its saved bind argument.
The author fixed that call; the fresh build-fixed-bind run exited0. Original
failure output and command status remain in build-verify; no failed run is hidden.

Fresh trusted/unrelated valid one-day P-256 CAs and SAN localhost/127.0.0.1
retained explicit CA/exact Origin https://127.0.0.1:8443 without trust bypass.
Library group:17passed/0failed/0ignored in7.04s;2922 unrelated library tests were
intentionally filtered. Integration: room17/0 in9.40s and pending2/0 in0.09s,
both zero ignored/filtered. Exact names and per-target summaries were checked.
Total36 is scoped evidence, not full-workspace or playable-player acceptance.

Library cargo-test PID2182599 exited0/joined. Integration test PID2184293
and owned relay PID2184292 exited0/joined; UDP56008 had process-owned readiness,
relay SIGINT0, no forced kill. Postflight confirms all three recorded process
groups absent, UDP port rebind, six synthetic private keys0600 and all seven
source hashes unchanged across the successful compile and both executions.
Each real library case asserts its server runtime disposal and thread join.

Evidence: target/wf/webtransport-server-commit-14130/.
- build-verify/invocations.json: preserved initial compile exit101.
- build-fixed-bind/invocations.json: current build/no-run exit0.
- library/invocations.json SHA256 c8e80af099d7d40f297d83e53e23de276d06a09525071fbcc3c304b7e6ff8517.
- library/tests.log SHA256 288ab0eae75393604a8aee3ec525db733cd4780b68447d8e43aee371b9dd7e60.
- integration/invocations.json SHA256 2b4a7063ea1571993f8f7d01cafc071fd564c602a2a55dda5621bb8ae27546ae.
- integration/tests.log SHA256 910cc5a356e0fdd7dda7ee32c73306f6346f88ee333610fd8cceea06409158b0.
- root-postflight.json: live source/native identities, original raw-log hashes,
  process/group/port checks and key modes.
- static-preservation.json: four old fixture source identities and exact new names.
- smoke/invocations.json: public serve-multiplayer help0 and invalid-option1.

New standalone rustfmt and whitespace checks passed. The old actor has
pre-existing import/child-fixture formatting debt recorded by the PLAN baseline;
it was not broadly formatted. Basic security review finds test controls compile
out and existing explicit CA/Origin/admission policy unchanged. No specialist
trust/dependency/UI boundary is introduced.

The current headless native artifact is223670608bytes, SHA256
5dd6238bef2e0666b77e37f1f7645fb3a95de92dbb9a19e1f3e6496a69123aaa.
It differs from the previous debug artifact and is not current GUI proof.
Browser artifacts were not rebuilt or revalidated here.

## Reproduce

Prepare fresh strict SAN PKI and set test-child BEATKERNEL_TEST_QUIC_CERT,
BEATKERNEL_TEST_QUIC_KEY, BEATKERNEL_TEST_QUIC_CA and
BEATKERNEL_TEST_WEBTRANSPORT_ORIGIN for library cases. Each owns its own fresh
server thread and nonzero loopback bind. For integration, separately start the
current relay with fresh strict PKI, exact Origin and --group-hosts 4; also set
BEATKERNEL_TEST_WEBTRANSPORT_URL and BEATKERNEL_TEST_QUIC_WRONG_CA in that
test child. Use the cached environment/flags above.

```sh
cargo test -p beatkernel-bms-runtime --no-default-features --features webtransport \
  --lib --locked --offline -j2 multiplayer_webtransport::group:: -- \
  --include-ignored --nocapture --test-threads=1
cargo test -p beatkernel-bms-runtime --no-default-features --features webtransport \
  --test multiplayer_webtransport_room_loopback \
  --test multiplayer_webtransport_connect_cancel --locked --offline -j2 -- \
  --include-ignored --nocapture --test-threads=1
```

Run the library and integration commands separately: the group filter would
otherwise exclude integration cases. The ignored local owned-relay.py runner
records the exact fresh credential/process recipe, names and cleanup bounds.

## Independent gates and retained scope

Formal independent review, subsequent independent qa-cli and the one owning
verify/close-or-park are still pending. These root results are not their PASS.
WBS12.09.03 remains V; overall136/489, remaining353 and audio12/27 are unchanged.
The full native Goal remains active and incomplete.

## Known ceiling

Complete Commit bytes with an outstanding actor receipt and the two observed
stale-event orders are proved; partially transmitted wire bytes, forced late
successful setup acquisition, other arbitrary ordering and post-final failures
are not. Unscoped real-terminal paths and injected fixture/start/cleanup/signal
failures remain source-reviewed gaps. The real stale terminal proves the outer
missing-resource guard; inner membership guard has separate portable evidence.
Rust thread join has no portable timeout; forced outer termination is failure.
Browser/native/crossOS/device synchronization, real gameplay/audio, performance
and full-player/release acceptance remain original unfinished obligations.
