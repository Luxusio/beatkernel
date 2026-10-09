# Retained-output practice

Native solo and local cohort play now support bounded loop/scrub controls on
the same output owner, Mixer, original PCM bank and converter. BGM repetition
runs in the audio render path; worker stalls do not require endpoint reopening.
The common pump installs fresh judge/gauge/score/capture state only after an
actual target-qualified, presented boundary and the original acquired input
prefix. Physical output time stays monotonic while the original song anchor
may move backward. F5 remains a separate joined fresh retry.

## Implemented behavior

- F8/F11 use the actual Player/desktop request and correlated response path.
  Pending, stale, unsupported and conflicting requests remain explicit.
- Original chart/audio/sound plans remain available for backward transitions.
  Initial native registration, competition and capture consistently use the
  selected source/chart pair; invisible/mine metadata retains its existing
  section-selection semantics.
- Raw source lookahead cannot authorize converted transitions. Ordered projected
  receipts use actual generated target samples, including cached PCM and held
  silence; pending projection prevents unsafe cold conversion transfers.
- Inputs between exact logical end and the presented Ceil boundary preserve
  acquisition and physical state without old-attempt judgment or replay Input.
  Equal-boundary input belongs to the fresh attempt.
- Applied retry/recording identity advances before fallible visual publication.
  UI responses consume their originating action atomically under contention.
- Stop admission evidence follows the retained Mixer's cumulative lifetime.
  Per-attempt barriers and failure latches reset independently.
- Retired captures are archived by the practice pump. Final live captures stay
  session-owned for existing native replay/result/score finishers, preserving
  the primary error and exclusive recording paths.
- Linux, Windows and macOS solo/local consumers connect the common mechanism.
  The actual Desktop spawned worker, common pump and production initial-output
  factory share one unique converted state in the connected journey fixture.

## Current evidence

Executable changes are committed as `f5196da` (audio kernel) and `22c95c9`
(player integration). `fe8bdbc` formats introduced integration code; the
coordinator's initial partial formatter experiment was discarded before builds,
and complete formatter groups were checked for missing/duplicated token content.

| Check | Actual result | Evidence under target/wf/ |
| --- | --- | --- |
| Full core, including doctests | 479 passed, zero failed; unchanged core behavior | gapless-projected-core-regression-cached-development.log |
| Strict core all-target Clippy | Passed | gapless-projected-core-fixed-clippy.log |
| Combined app library development | 2250 passed, zero failed, six ignored | gapless-initial-source-full-app-development.log |
| Native desktop binary development | 307 passed, zero failed, five ignored | gapless-initial-source-full-nativebin-development.log |
| Fresh full DEEP code/security review cycle4 | Both PASS, no findings | Current task review receipts |
| Independent Windows all-target typing | Exit0, Finished | qa-cli-gapless-01a1220c-1/windows-alltargets.log |
| Independent macOS all-target typing | Exit0, Finished on cached retry | qa-cli-gapless-01a1220c-1/macos-alltargets-cached.log |
| Independent public core practice debug/release | 27 passed each, zero failed | qa-cli-gapless-01a1220c-1/core-practice-debug.log and core-practice-release.log |
| Independent current app library after formatting | 2250 passed, zero failed, six ignored | qa-cli-gapless-01a1220c-1/app-full.log |
| Independent current native desktop binary | 307 passed, zero failed, five ignored | qa-cli-gapless-01a1220c-1/native-desktop-full.log |
| Independent Linux/main build and direct CLI | Build exit0; 7/7 CLI checks, literal 1000 mono zero frames/4000 bytes | qa-cli-gapless-01a1220c-1/linux-cli-build.log and cli-results-final.json |

Independent qa-cli returned PASS for AC001–006 on frozen source fe8bdbc.
Documentation review and receipt-backed task verification passed; the child
TASK__gapless-practice-output-timeline is closed.
WBS09.11 software E2E is D: 86/193 (44.56%);09.12 physical acceptance remains E.
The full 193-leaf Goal remains active.

The terminal Windows development and initial macOS QA typing attempts timed
out124 without a Finished result; they are not PASS. Their final cached checks
above supersede them for Rust type evidence only. Foreign checks use stub C
compilers and prove neither native linking nor driver operation. Baseline
unused/dead-code warnings, macOS block future-incompatibility and unrelated
formatting debt remain separately recorded; no global format/lint PASS is claimed.

## Reproduction

Use Rust1.98.1 via `source target/toolchain/env.sh`, the cached target directory
`target/wf/worklet-chronology-qa-cli-1/cargo`, jobs1/incremental0/dev+testdebug0
and existing host CC/AR. Compiler commands use timeout240s/kill10s and AS8GiB;
one compiler owner runs at a time. Do not apply compiler memory limits to
Node/browser/GUI processes.

```sh
cargo test -p beatkernel --locked --test practice_program
cargo test -p beatkernel --release --locked --test practice_program
cargo test -p beatkernel-bms-runtime --features desktop,webtransport --lib --locked
cargo test -p beatkernel-bms-runtime --features desktop,webtransport --bin beatkernel-bms-runtime --locked
```

The connected tests use actual Mixer/converter/owner state and injected native
observations with independent PCM/time/capture oracles. They cover partial
target admission, autonomous loops, disable/scrub, F5 join/retry, exact and
subframe cuts, failure-owned Stop execution, contention and final serialization.

## Known ceiling

Portable PCM and injected native-owner evidence do not prove physical acoustic
sync, actual ASIO SDK/driver operation, hardware latency, browser practice parity
or world-leading performance. These remain in the full player WBS, including
09.12 and the OS/device/browser matrix; no component-only result completes09.11.

Output replacement during retained practice is currently refused before owner
retirement or request remapping. Converted rate/backend replacement needs a
proven bridge for pending projection and unread PCM. Watch/network practice is
not enabled by this task. Default no-argument desktop GUI launch on this host
cannot load libxkbcommon-x11.so; the injected Desktop journey does not prove a
native window/device run. QA retains that diagnostic as main-empty.stderr and
does not claim the whole native GUI/device matrix. See [the retained practice contract](../kernel/REQ__gapless-practice.md)
and [fresh-owner restart contract](../kernel/REQ__section-restart.md).
