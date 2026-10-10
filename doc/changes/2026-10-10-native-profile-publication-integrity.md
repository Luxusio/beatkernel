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

## Verification in progress

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
Current whole-app regression, independent source/security review, final QA and
documentation review are pending. Earlier record-publication test results are
baseline evidence, not acceptance of this changed source. WBS09.13 remains V;
the full product retains 193 active leaves and 87 completed leaves.

## Known ceiling

Known ceiling: 동시 replacement 잠금·디렉터리 전원 장애 복구는 보장하지 않음 — 전체 WBS09.13 구현 시 확장.

Directories remain caller-owned and stable. Separate path checks and opens do
not prove hostile ancestor containment. File synchronization does not prove
directory-entry power-loss durability; best-effort cleanup can leave a stage.
Injected capacity refusal is not actual disk exhaustion, and cross-target Rust
typechecks are not Windows/macOS execution. Process interruption/recovery and
all remaining player requirements retain their original scope.
