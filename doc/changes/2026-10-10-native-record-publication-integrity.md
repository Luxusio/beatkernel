# Native record publication integrity

Native replay and completed-result saving now share a file publisher that writes
an exclusively owned sibling stage, flushes and synchronizes it, closes the
writer, then creates the final name with an exclusive hard link. Existing files
and symlinks remain untouched. Failures before publication expose no partial
final record. Cleanup preserves the original error or committed success. Replay
encoding remains in the capture layer; filesystem operations live in its native
adapter. Public save signatures, canonical bytes, and archive group
prepare-all/attempt-all/first-error behavior remain unchanged.

## Independently executed software verification

The independent qa-cli agent executed the following commands against source
revision `01ed6a0`, after fresh DEEP code and security reviews returned PASS.
Every final command exited zero. Logs are under
`target/wf/qa-cli-records-01a1235f-1/`; that ignored directory is local evidence,
while the test sources and commands below are durable, reproducible artifacts.

| Verification | Actual result |
|---|---|
| `cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked` | 2,295 passed, 0 failed, 6 ignored |
| `cargo test -p beatkernel-bms-runtime --test replay_capture --features desktop,webtransport --locked` | 17 passed, 0 failed, 0 ignored |
| `cargo test -p beatkernel-bms-runtime --bins --features desktop,webtransport --locked` | 466 passed, 0 failed, 9 ignored across seven binaries; main is 307 passed / 5 ignored |
| `cargo check -p beatkernel-bms-runtime --lib --no-default-features --locked` | exit 0 |
| `cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser,browser-audio --locked` | exit 0 |
| `bash target/toolchain/xcheck.sh windows -p beatkernel-bms-runtime --lib --features desktop,webtransport` | exit 0; C/C++ stub, Rust type evidence only |
| `bash target/toolchain/xcheck.sh macos -p beatkernel-bms-runtime --lib --features desktop,webtransport` | exit 0; C/C++ stub, Rust type evidence only |
| Production helper compiled with `rustc --test --edition 2021 app/src/native_publication.rs`, then executed | 25 passed, 0 failed, 0 ignored |
| Actual-helper WASM fixture compiled and executed with Node as shown below | 3 passed, 0 failed |
| Changed Rust files formatted with `skip_children=true`, excluding inherited whole-`lib.rs` formatting debt; base diff whitespace check | exit 0 |
| `python3 tools/wbs_status.py --json` | 87 / 193 complete, 45.08%; WBS09.13 remains V |

Use the established host toolchain environment and cached build configuration
described by [the development guide](../common/GUIDE__parallel-development.md).
Standalone native compilation additionally uses the configured host linker.
The durable WASM regression uses the production helper without host imports:

```sh
rustc --crate-type cdylib --target wasm32-unknown-unknown --edition 2021 \
  app/tests/fixtures/wasm_native_publication.rs -o /tmp/native-publication.wasm
BEATKERNEL_NATIVE_PUBLICATION_WASM=/tmp/native-publication.wasm \
  node --test app/tests/wasm_native_publication.test.mjs
```

The helper fixtures use real files and exclusive links, concurrent publishers,
and static injection into the production `write_all` path for
short/Interrupted/zero/partial writes, flush/sync refusal and cleanup refusal.
Public replay tests validate canonical accepted prefixes, identity and returned
byte counts. Thirteen archive-consumer tests verify actual store/solo/member
files, valid reserved-name refusal, complete concurrent winners and original
first-error/all-attempts behavior. The standalone helper run repeats tests
included in the full library run; these counts are not unique test totals.

The first replay integration build reached its actual 240-second compiler
timeout before assertions ran. Its log remains `replay-timeout-first.log`.
The same command's cache continuation executed all 17 tests and exited zero.
The timeout is not counted as a test PASS or assertion failure. Existing build
warnings and inherited formatting debt remain; no strict whole-app lint or
format PASS is claimed. Ignored cases remain unverified.

## Corrections verified during development

Pre-review inspection changed stages to uppercase hexadecimal 8.3 names with
checked 44-bit identities and conservative native alias exclusion.
Filename assumptions were checked against Microsoft's
[file naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file),
[8.3 format](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/18e63b13-ba43-4f5f-a5b7-11e871b71f14)
and [space/period normalization](https://learn.microsoft.com/en-us/troubleshoot/windows-client/shell-experience/file-folder-name-whitespace-characters).

The first formal code review found that an accepted final basename could be
another publisher's stage. An actual production-generator probe reproduced
partial bytes at that requested final and deletion by normal stage cleanup.
The correction reserves exactly the generated hexadecimal 8.3 namespace and
its native aliases, returning `InvalidInput` before filesystem effects.
Ordinary `.bkr` and `.bkresult` destinations remain accepted. Regression tests
cover absent/existing reserved finals, refused-cleanup partial stages and
successful concurrent publication to distinct accepted destinations.

An exact-helper WASM probe exposed a process-ID trap before unsupported file
I/O. The helper now uses a zero process seed on wasm32 while native targets
retain their PID seed. The existing public API remains available. The durable
WASM fixture executes the actual helper with no imports and verifies ordinary
`Unsupported`, reserved-name `InvalidInput`, and absence of traps.

Commit-backed learning: both verified corrections are captured in the owning
publication/replay contracts and executable regression fixtures. The
checkpoint-specific optional Git-lock workaround is rejected as a durable
product change; it does not change the player or this publication contract.

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
