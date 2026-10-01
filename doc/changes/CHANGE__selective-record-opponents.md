# Selective saved-record opponents

Records now shows selected-path own/other occurrence counts and provides Remove
Own and Remove Other controls. Each clears the first exact kind/path occurrence
in the parent draft, preserving duplicate entries, other kinds, unrelated
options and row identities. Cleared rows remain reusable. Removal works without
reopening or previewing the selected file and never deletes a record.

The selected parent settings editor refreshes after removal so its old text
cannot restore the path. Apply remains separate and accepted native arguments
stay pinned. Hidden, pending or closing Records cannot mutate the draft; absent
membership disables its corresponding control and missing targets preserve
other values. Invalid paths reject before mutation.

Retained membership and button gates update independently from catalog rows.
An overfull manually edited draft stays visible and removable, while Add is
disabled at eight. Frame counts remain bounded by the existing 128 settings
rows and selected occurrences cannot exceed the actual total. Native Apply and
accepted competition retain their existing limits.

Prepared fixtures cover mixed kinds, Unicode/literal paths, repeated records,
first-occurrence order and row preservation, invalid/missing targets and reuse
at capacity; actual retained membership/hit/pending/atomic-error behavior; and
desktop parent-editor refresh, accepted options and hidden-child guards.
Tests and native GUI execution remain deferred; compilation is source evidence.

Known ceiling: matching uses literal UTF-8 paths, without filesystem identity
resolution. Equivalent path spellings may therefore appear as separate records.
Only selected catalog entries have per-kind removal controls; Clear All and
the parent settings editor remain available for records outside that catalog.

Source checks with Rust 1.98.1 succeeded for the host workspace/all-targets,
Windows GNU and macOS application/all-targets, headless application/all-targets,
and WASM graphics library with locked dependencies. The existing retained error
fixture was updated for the added nodes and compiled again on the host. Scoped
formatting and diff checks succeeded. Fixtures were compiled, not executed;
native GUI and formal acceptance remain unverified.
