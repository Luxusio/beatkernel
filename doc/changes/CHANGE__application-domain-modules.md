# Organize gameplay output by domain and effect boundary

Ten flat output modules now have their canonical implementation under
`gameplay/output`: pure command state in `domain`, orchestration in `application`,
effect contracts in `ports`, and Player/native implementations in `adapters`.
OutputUiPort and OutputReplacementBackend are extracted from use-case bodies;
PlayerOutputUi is extracted from command orchestration. Application/domain code
has no direct Player or platform imports. Wider timing/session boundaries and
other contexts remain an incremental migration, not a completed architecture.

Existing root paths are compile-time re-exports of the same types. No new crate,
runtime wrapper, per-note allocation/lock or dynamic dispatch was introduced.
Native adapter cfg gates are retained. Twelve fixture files are colocated with
their implementation or grouped under `gameplay/output/tests`; their bodies
match the previous HEAD after formatting and one relocated include path.

Workspace all-targets compilation passed with only existing warnings. The
focused output domain tests executed: 29 passed, 2 failed. Both failures match
the prior verify-result baseline (replacement tests expecting 1.125 from an
output Mixer which clamps to 1.0); assertions were not altered during relocation.
The full library suite executed: 1454 passed / 100 failed. The main executable
suite executed: 199 passed / 18 failed. All failures are named in the supplied
prior verification record after mapping the relocated module prefixes; no new
failure was observed. Reduced test debug information avoided the earlier LLVM
output allocation failure without changing assertions or optimization level.
WASM browser and browser-audio compilation, headless all-target WebTransport
compilation and Windows GNU/macOS all-target Rust checks all exited zero.
Available cross-target scripts use C stubs and prove Rust
types only; they do not compile/link SDK code or establish device/ABI accuracy.

The [domain module decision](../kernel/ADR__application-domain-modules.md) records
the user-approved direction. Independent formal review/QA and full Goal
completion remain outstanding. New parallel agents await another user parallel
instruction; this refactor and existing-fixture execution run in the coordinator.
