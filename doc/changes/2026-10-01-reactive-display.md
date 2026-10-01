# Retained Display settings and reusable nodes

The actual Display route now owns one retained reactive view per Navigator
instance. GPU backend, present mode, FPS and lookahead have independent editor
signals, with selection focus affecting only the old/new fields. Errors and
buttons update separately. Borrowed frames compare before cloning changed
editors; pending state removes every hit. Existing validation, Done/Back,
Settings Apply and save/restart backend semantics remain. Failed Done preserves
the child and parent drafts. Exiting the child or Closing disposes its scope.

Display uses a reusable RetainedNodes primitive for static/dynamic nodes,
immutable geometry packets, dirty state and ordered composition. The view owns
Floem scope disposal; the primitive has no navigation, native resources, clocks,
custom signals or batching scheduler. Display enters event-driven Wait using
the existing hit invalidation, metadata wake and surface retry boundaries.
The old imperative Display renderer is replaced; existing layout fixtures use
a test adapter to the actual retained view.

Known ceiling: earlier Selection, Practice and Settings views still contain
their original packet helper implementations; migrating them to this primitive
is separate work. Records, Players and Devices still need reactive migration.
Changed nodes still cause full packet concatenation and rectangle uploads;
there is no performance benchmark. The existing fixed960x720 viewport and
native supported-presentation admission remain.

Pure view/node and desktop draft/lifecycle fixtures were authored and compiled
only. Tests/product/GUI/native/GPU/shader/device/file/network execution,
benchmarks and independent formal review/security/QA remain user-deferred.
The full Goal and Harness task remain open without an acceptance PASS.

Rust 1.98.1 app all-target source compilation succeeded for Linux, Windows GNU
and macOS. Headless all-target and WASM graphics-library checks succeeded.
Scoped rustfmt and git diff --check succeeded. Existing macOS block0.1.6
future-incompatibility and WASM cadence dead-code warnings remain.
