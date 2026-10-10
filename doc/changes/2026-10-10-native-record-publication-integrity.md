# Native record publication integrity

Native replay and completed-result saving now share a file publisher that writes
an exclusively owned sibling stage, flushes and synchronizes it, closes the
writer, then creates the final name with an exclusive hard link. Existing files
and symlinks remain untouched. Failures before publication expose no partial
final record. Cleanup preserves the original error or committed success. Replay
encoding remains in the capture layer; filesystem operations live in its native
adapter. Public save signatures, canonical bytes, and archive group
prepare-all/attempt-all/first-error behavior remain unchanged.

## Verification in progress

Final development verification: app library filter `publication` with
`desktop,webtransport`, 115 passed, 0 failed, 0 ignored. This includes all
21 `native_publication::fixtures` and 11 native archive consumer tests, plus
existing publication regressions. The tests exercise actual files, exclusive links and
concurrent publishers, with static injection for short/Interrupted/zero writes,
partial write refusal, flush/sync refusal, and cleanup refusal. Public replay
integration with `--no-default-features`: 14 passed, 0 failed, 0 ignored,
including seven new consumer tests. Independent code/security review and QA
remain pending. This document does not claim task completion. An earlier
desktop/webtransport integration build reached its 240-second compiler timeout
without executing assertions; it is not reported as a test PASS.

Pre-review inspection also corrected staging/final filename aliasing. Staging
now uses uppercase hexadecimal 8.3 names and checked 44-bit identities, with
conservative case/space/period/stream-base exclusion before creation. The initial
20-test run predates this correction; the 115-test run includes its regressions.
Filename assumptions were checked
against Microsoft's [file naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file),
[8.3 format](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/18e63b13-ba43-4f5f-a5b7-11e871b71f14)
and [space/period normalization](https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/file-folder-name-whitespace-characters).

The first formal code review found a distinct-destination namespace overlap:
one accepted final basename could be another publisher's stage. A standalone
probe using the actual production generator and two publishers reproduced
three partial bytes at the other requested final before either commit, followed
by `AlreadyExists` and normal staging cleanup. The correction reserves exactly
the generated hexadecimal 8.3 namespace and its native aliases, returning
`InvalidInput` before filesystem effects; ordinary `.bkr` and `.bkresult`
destinations remain accepted. The corrected helper's 25 tests pass when compiled
directly with `rustc --test app/src/native_publication.rs`, demonstrating its
standalone native-IO boundary. The corrected app library `publication` filter
with `desktop,webtransport` also passes 121 tests, including the 25 helper and
13 archive consumer tests. Corrected public replay integration with
`--no-default-features` passes 17 tests. Fresh independent review/QA remain
pending; development checks do not establish task completion.

## Known ceiling

- Known ceiling: cleanup refusal may retain an owned staging file — upgrade when
  broader orphan-recovery acceptance requires it.
- Known ceiling: Hard-link 미지원 파일시스템은 저장을 거부하고 staging 정리는
  best effort — 더 넓은 파일시스템 지원이나 디렉터리 crash durability 요구가
  생기면 별도 구현·검증 필요.
- Known ceiling: 개별 파일 게시만 원자적이며 그룹은 비트랜잭션 — 그룹 트랜잭션
  요구 시 확장.
- Known ceiling: staging 정리는 best effort이며 디렉터리 전원 장애 내구성은
  보장하지 않음 — 전체 저장 내구성 작업에서 검증.
- Known ceiling: native Windows runtime evidence remains unavailable — upgrade
  verification when a Windows runner is available.

Directories remain caller-owned. Hostile ancestor changes, directory-entry
power-loss durability, actual disk exhaustion and Windows/macOS execution are
not established by Linux software fixtures. Settings/profile storage is outside
this change. Whole WBS09.13 remains open and the full product WBS keeps its
193 active leaves.
