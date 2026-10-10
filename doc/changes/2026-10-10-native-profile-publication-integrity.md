# Native profile publication integrity

Profile saving now uses the shared complete-file publisher, preserving canonical
v1/v2 bytes, same-host/version validation and create-versus-replace behavior.
Pure codecs/models remain in `settings_profile.rs`; filesystem loading, target
validation and saving belong to its private `native_settings_profile` adapter.
The four existing public save/load functions remain available through reexports.
The [profile publication contract](../runtime/REQ__native-profile-publication.md)
owns validation precedence and storage guarantees.

## Actual defect reproduction

Before correcting profile saving, an isolated process called the actual public
`save_profile` API with a valid model and a filename equal to its initial
production stage: `.beatkernel-profile-{pid}-0.tmp`. Saving returned the original
`AlreadyExists` I/O error, and normal staging cleanup removed the requested final.
The compiled public probe exited zero after asserting this original failure;
logs are under `target/wf/native-profile-publication-integrity/`.
The probe observed the refusal and cleanup, while source inspection establishes
that writes preceded the self-link. It does not claim a concurrent observation
of transient partial bytes.

## Measured verification

The shared publisher extension has passed 28 standalone native tests and three
actual-helper WASM Node tests. The original 25 native tests are retained; three
new cases verify opaque commit-error identity, linked commit after complete
write/closure, and moved-stage ownership retirement with a foreign entry at the
released stage name preserved. WASM still returns ordinary `Unsupported` and
reserved-name `InvalidInput` without imports or traps.

Focused app library execution with `--no-default-features` passed all 21 profile
tests, including the 11 new adapter fault/concurrency cases and ten preserved
codec/file scenarios. Public integration with `--no-default-features` passed
all ten new profile tests and all 17 preserved replay-capture tests. Two retained
legacy candidate wrappers are now test-only, avoiding new production dead-code
warnings; their signatures and fixture behavior are preserved.
Source `1920781` passed independent DEEP code and security reviews before
independent CLI QA executed the following commands. Evidence is under
`target/wf/qa-cli-profiles-01a123b7-1/`; these ignored logs may be absent in a new
checkout, so use the repository's toolchain instructions and rerun the commands
rather than infer acceptance from missing logs.

| Command / evidence | Actual result |
| --- | --- |
| `cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked` / `app-lib-1.log` | 2,309 passed, zero failed, six ignored |
| `cargo test -p beatkernel-bms-runtime --test settings_profile_publication --test replay_capture --features desktop,webtransport --locked` / `integration-2.log` | Ten public profile and 17 replay tests passed, zero failed/ignored |
| `cargo test -p beatkernel-bms-runtime --bins --features desktop,webtransport --locked` / `bins-1.log` | 466 passed, zero failed, nine ignored |
| `cargo check -p beatkernel-bms-runtime --lib --no-default-features --locked` / `model-1.log` | Exit zero |
| `cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser,browser-audio --locked` / `browser-1.log` | Exit zero |
| `bash target/toolchain/xcheck.sh windows -p beatkernel-bms-runtime --lib --features desktop,webtransport` and the same command with `macos` / `windows-1.log`, `macos-1.log` | Exit zero; stub Rust typechecks only |
| `rustc --test --edition 2021 app/src/native_publication.rs` with the established host linker, then run the produced binary / `helper-run.log` | 28 passed, zero failed; repeated subset of the app library |
| `rustc --crate-type cdylib --target wasm32-unknown-unknown --edition 2021 app/tests/fixtures/wasm_native_publication.rs -o <artifact>` then `BEATKERNEL_NATIVE_PUBLICATION_WASM=<artifact> node --test app/tests/wasm_native_publication.test.mjs` / `wasm-helper-run.log` | Three actual helper WASM tests passed, zero failed |
| `rustfmt --edition 2021 --config skip_children=true --check` on the six changed Rust files / `format.log`; `git diff --check cbd5b8c..1920781` / `diff-check.log` | Exit zero |
| `python3 tools/wbs_status.py --json` / `wbs.json` | 87 completed / 193 active; whole09.13 remains V |

The first integration compilation reached its actual 240-second guard with exit
124; the exact same command continued using the build cache and exited zero.
No test failure was observed, and the other commands required no continuation.
Existing compiler warnings are not a strict whole-app lint PASS. Public profile
tests execute the original save/load entry points on actual native files;
helper WASM tests do not establish direct public profile WASM runtime acceptance.

These executed results are separate from the Harness receipt-backed close gate.
Earlier record-publication results remain baseline evidence. Whole WBS09.13
still requires interruption/recovery and its remaining storage criteria; the
full product retains 193 active leaves and 87 completed leaves.

## Known ceiling

Known ceiling: 동시 replacement 잠금·디렉터리 전원 장애 복구는 보장하지 않음 — 전체 WBS09.13 구현 시 확장.

Directories remain caller-owned and stable. Separate path checks and opens do
not prove hostile ancestor containment. File synchronization does not prove
directory-entry power-loss durability; best-effort cleanup can leave a stage.
Injected capacity refusal is not actual disk exhaustion, and cross-target Rust
typechecks are not Windows/macOS execution. Process interruption/recovery and
all remaining player requirements retain their original scope.
