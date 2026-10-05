# Completed-result archive foundation

Completed live results need a storage representation independent of the UI,
audio device and replay-prefix metadata. The common archive associates the
whole original player roster with each replay header, actual gauge policy and
final full-song or practice result. Reading returns historical archive values;
it cannot manufacture live completion evidence or authenticate an editable file.

The business codec and save/load policy have no filesystem, clock, network or
renderer dependency. The storage port receives the complete encoded archive
only after validation. The native adapter uses a caller-selected directory,
bounded reads and exclusive creation. Write/flush failures may leave a partial
new file, and flush does not establish crash or power-loss durability.

Version 1 stores identity, extent, gauge policy and outcome. It does not store
rich score/timing/comparison data. High-level native replay-save integration,
browser IndexedDB association and catalog/UI loading remain subsequent work.
The existing replay-save path does not yet automatically create this archive.

## Known ceiling

High-level native save and browser integration, rich score/timing metadata and
crash-atomic durability remain unfinished. The standard-library native adapter
does not protect against concurrent filesystem path replacement; callers must
own the selected directory and its mutation policy.

Independent authors supplied ten deferred groups: six codec/policy and four
scripted-storage groups. They cover literal golden bytes, every truncation,
later-row corruption, 1..64 original IDs, header-only envelope refusal, custom
thresholds/grade overrides, signed integer bounds, 20-hour/week practice extents,
one-effect save ordering, read bounds and original storage errors.

After both writers' terminal stop, scoped rustfmt and `git diff --check`
succeeded. These four compile-only commands returned exit 0:

```
cargo check --workspace --all-targets --features beatkernel-bms-runtime/webtransport --locked
cargo check -p beatkernel-bms-runtime --no-default-features --features webtransport --all-targets --locked
cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser --locked
cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser-audio --locked
```

Existing dead-code warnings remain. No test assertions executed. Real
filesystem/browser/device acceptance, crash recovery and formal review/QA
remain deferred; these compile results do not establish those requirements.
