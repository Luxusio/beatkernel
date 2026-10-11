# Retained cohort transition stress

Status: source1712136 passed independent DEEP code/security reviews and actual
CLI QA. Final durable documentation review and Harness close remain pending.
The first
integration execution reported six passing and five failing tests. Correcting
the workload setup and terminal-boundary accounting produced the current
passing run; this is coordinator development verification, not independent QA.
The first release build reached its240s process deadline while compiling
dependencies (exit124, no reported source error). Its log is preserved as
`release-build.log`; the cache-reusing follow-up `release-build-cached.log`
finished successfully in2m56s.
The complete player Goal and full WBS13.05 remain unfinished.

The [contract](../kernel/REQ__cohort-transition-stress.md) fixes bounded true
member/attempt counts and requires existing production solo/cohort pumps,
original member recordings and actual retained Mixer PCM/receipts. Generated
ports model virtual acquisition/output lifecycle, without native hardware or
latency claims. Public pumps retain prefix retirement, archive, practice
replacement and identity publication; the workload must not reproduce those
private commit sequences or fabricate their successful acknowledgements.

Independent planning inspection established that cohort pumps accept2..64
members, so one-member coverage must use the existing solo pump. It also
established explicit application refusal of output replacement during retained
practice. A low-level replacement would bypass that policy. The child tests
that refusal and separately admitted successful acknowledged replacement;
it makes no successful interleaving claim and changes no admission rule.

## Development verification

The actual command
`cargo test -p beatkernel-bms-runtime --no-default-features --locked --test cohort_transition_stress`
compiled in17.56s and ran11 tests in0.16s, all passing. The member matrix
uses1/2/3/4/64 members, three attempts and three notes per member, plus one-note
and single-attempt cases. Tests compare original metadata, canonical replay,
live and reconstructed judges, literal full PCM, backward capture song times,
monotonic physical/playback frames, retained owner identity and genuine pause
and replacement observations. Failure cases cover atomic last-member refusal,
late/incomplete input, queue/receipt exhaustion, archive/presentation refusal,
unacknowledged pause and replacement timeout.

Raw logs are under
`target/wf/cohort-transition-stress-development-12bfe/`: `first.log` records the
missing default C compiler, `compiler-env.log` the two source API errors,
`fixed-api.log` the initial runtime failures, `fixed-setup.log` the9/11 result,
and `fixed-terminal.log` the11/11 result. These ignored local logs are not
portable committed evidence. The existing private compiler works with
`source target/toolchain/env.sh`, `CC="$PWD/target/toolchain/host-cc.sh"`,
`AR="$PWD/target/toolchain/host/usr/bin/x86_64-linux-gnu-ar"` and
`CARGO_BUILD_JOBS=1` supplied to Cargo; no system installation was performed.

The test originally treated `RenderReport.song_position` as an advancing song
cursor. Production Mixer stores a seek/practice anchor in that field. The
corrected assertion proves rewind through literal prior section-end and next
attempt input song times while physical/playback frames advance; real render
reports remain unchanged. Likewise, a terminal inactive Ended control
generation is distinct from another active attempt.

The optimized example passed actual help, default2/8/8, solo1/1/1 and
64-member3-attempt3-note executions, plus an identical repeated64-member
stdout comparison. Duplicate, missing, unknown, out-of-range and mixed-help
arguments all exited1 without a success banner. Original stdout/stderr and
`development-cli-results.json` preserve all10 command outcomes in the same
local evidence directory. This confirms repeatable software observations,
not native latency, physical playback or independent acceptance.

## Independent acceptance

Both selected defect hunters returned empty candidate arrays. The formal code
review and independent security review each returned PASS with zero findings
against all six committed files at1712136. Actual QA began after both finals
and passed fresh11 integration tests (6.44s compilation,0.16s execution), scoped
rustfmt and current optimized build (6.52s).

The independent CLI matrix passed30 direct executions:12 help/happy/edge paths
and18 invalid paths. It includes default2/8/8,1/2/3/4/64 members with repeated
mixed notes, identical repeated64 stdout, single attempt/note, one-note repeated
attempts, cycles64 and notes64 separately. Invalid arguments exited1 with
diagnostics and no success stdout. No exhaustive maximum cross-product,
physical/native/GPU/network or long wall-clock acceptance is asserted.

`target/wf/cohort-transition-stress-qa-12bfe-1/` preserves `tests.log`,
`format.log`, `build.log`, `identity.json`, `cli-results.json`, `wbs.json` and all
original stdout/stderr. The optimized artifact was3236848 bytes with SHA256
`da9ddf6fc9b0042d02db02e5dc7ad73d9489ec8a94f00c8d37eb7d0476e48b28`,
unchanged throughout execution. These ignored artifacts remain local evidence.
All QA sessions58890/85580/53091 terminated successfully and leases were
released. Existing library/bin and new example test-helper dead-code warnings
remain; scoped formatting and required commands passed. Global WBS remains
91/193(47.15%); full13.05/13.08 and player Goal remain unfinished.

## Known ceiling

Virtual lifecycle snapshots derive from actual Mixer renders and exercise
production qualification, but do not prove real native devices, clocks, worker
retirement or physical sound. Retained Box<Mixer> allocation and complete owner
transfer are observable; private bank/consumer addresses are not inspected.
True64 members and repeated software transitions do not establish64 remote
hosts, GPU performance, comparative ranking or long wall-clock soak.
