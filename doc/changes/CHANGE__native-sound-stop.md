# Native failed-player scheduled voice stop

Common native solo/cohort report observation now uses the existing scheduled
gameplay voice-stop component after consuming independent gauge/capture/score/
competition/presentation observations. Accepted Stop commands and exact queue
errors are appended to the actual failure report; the core input/judge/Play
prefix and original GroupError failure identity remain readable.

Local observation accepts a mutable report prefix. All four actual input/deadline
success and GroupError callers pass that prefix, and every newly failed member is
fenced/stopped after whole-prefix observation. Poison remains poison, numeric
failure with accepted stops does not abort healthy members, and BGM/other member
voices remain outside the failed member's stop namespace. No automatic retries
or global output control are added. Unattached execution uses the same path;
the UI remains an independent observer.

Four independently authored native groups use actual common publish/process/
observe paths and retained test fixtures, covering headless/attached policy,
future Play scheduling, source-aware post-fence inputs, BGM/survivor voices,
partial/full queue refusal and simultaneous capture/presentation/core errors.
Inactive-voice unknown_stops diagnostics remain visible. Existing four gauge
groups retain their judge/capture invariants with mutable arguments and exact
Stop/PCM evidence aligned. Two test-only helpers become pub(super) to share
the same real setup without adding a production seam.

Both writers delivered terminal Writes STOPPED before scoped formatting and
compile checks. Assertions and native target/device/browser/GPU
execution remain deferred. Replay/offline audio wiring, failed-session output
completion and clear/fail results remain unfinished. Mine admission stays guarded.

Scoped six-file Rust formatting and whitespace checks succeeded. The four
authorized compile-only checks all exited zero: workspace/all-targets WebTransport,
headless/all-targets WebTransport, WASM browser and WASM browser-audio. Existing
unused-code warnings remain in audio cadence and the playfield progress helper.
Host compilation includes the native fixture sources without running assertions;
it does not establish Windows/macOS target acceptance or native hardware output.
