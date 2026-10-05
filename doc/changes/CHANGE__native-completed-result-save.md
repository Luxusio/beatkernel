# Native completed-result save integration

Native recording needs to retain a completed-result archive as well as the
accepted replay prefix. The shared finalizer takes actual typed completion and
the original capture headers/gauge policies after native cleanup attempts.
Completion retained in a publication error remains usable for saving; a
cancelled, interrupted or merely recorded prefix does not become completion.

The Windows, macOS and Linux solo/local source paths append `.bkresult` to the
configured base replay
filename for solo and for the whole original local roster. All replay save
attempts precede the archive save callback. Original gameplay/cleanup errors
retain precedence. Native filesystem publication is exclusive-create and
preserves the path's native filename. Finite native capture setup now retains
its actual end in the canonical header.

The user's zero-cost abstraction preference is recorded in
[the performance requirement](../kernel/REQ__performance-and-testability.md).
Generic ports and typed values are preferred on hot paths; no extra per-note
dynamic dispatch, allocation or locking is required by the layer boundary.
This is an implementation rule, not measured proof of zero overhead.

## Known ceiling

Browser IndexedDB association, native/browser catalog/UI archive loading,
rich score/timing/comparison metadata and crash-atomic durability remain later
work. Windows/macOS target and real filesystem/device acceptance remain
deferred. No execution or formal review/QA is claimed.

## Compile-only evidence

After both writers' terminal stop, scoped rustfmt and `git diff --check`
succeeded. Ten independently authored deferred groups cover seven
save/finalization cases and three pristine capture cases. The save fixtures use
internal known-gauge completion construction; they do not execute native pumps.
These four commands returned exit 0:

```
cargo check --workspace --all-targets --features beatkernel-bms-runtime/webtransport --locked
cargo check -p beatkernel-bms-runtime --no-default-features --features webtransport --all-targets --locked
cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser --locked
cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser-audio --locked
```

Existing dead-code warnings remain. No assertions, filesystem writes,
Windows/macOS target checks or hardware/browser tests executed. Compilation
does not establish runtime behavior, zero abstraction overhead or reliability.
