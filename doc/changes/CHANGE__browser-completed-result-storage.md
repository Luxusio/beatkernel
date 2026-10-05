# Browser completed-result persistence

Browser recording needs to retain the common completed-result archive as well
as accepted replay bytes. Actual stepped completion, original capture headers
and actual gauge policies supply the archive. Export runs on the Worker before
replay consumption or gameplay release. A prefix, cancelled session, replay
playback or JavaScript completion label cannot supply completion evidence.

The integration transfers one bounded whole-roster archive to the Window and
associates its opaque bytes with each captured original player recording.
IndexedDB saves the replay and optional archive within one existing transaction,
validates their storage association and counts both payload sizes against the
library budget. Old recordings without archives remain readable. Loaded data is
historical and untrusted; the Window does not decode the business format.

## Known ceiling

Loaded-result presentation and native/browser catalog association still need
Rust decoding and identity checks. Version 1 omits rich score/timing/comparison
data. Generated binding execution, browser/IndexedDB interoperability, actual
allocation/performance and crash/power-loss durability remain unverified.
Async room archive drain and full Window lifecycle remain runtime coverage gaps.
Test execution, JavaScript parsing and formal review/QA remain deferred under
the existing user instruction.

## Compile-only evidence

Both writers supplied terminal stop before scoped Rust formatting and checks.
Twelve independent deferred groups were authored: four Rust Step/Mixer groups,
four actual-source Worker VM groups, three injected IndexedDB groups and one
pure Window archive-admission group. Existing Worker mocks received only the
required new export method. Rust fixtures cover every roster size 1..64.
No JavaScript parser, assertion, generated binding or browser was executed.

Scoped rustfmt and `git diff --check` succeeded. These four compile-only
commands returned exit 0, with existing dead-code warnings:

```
cargo check --workspace --all-targets --features beatkernel-bms-runtime/webtransport --locked
cargo check -p beatkernel-bms-runtime --no-default-features --features webtransport --all-targets --locked
cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser --locked
cargo check -p beatkernel-bms-runtime --lib --target wasm32-unknown-unknown --no-default-features --features browser-audio --locked
```

Compilation does not establish JavaScript behavior, real IndexedDB atomicity,
generated bindings, performance or browser/device acceptance.
