# Scoped panels and desktop navigation

The graphical player now dispatches input and drawing from one typed Navigator
route. Settings, Display, Records, Players and Devices drafts live in typed
PanelScopes; the actual retained back stack preserves parent instance IDs and
draft edits on Back. Child disposal cancels task permits before parent disposal.
Metadata queries, profile work and record scans/previews capture their initiating
instance and check cancellation before/after work; only the active initiating
instance may receive their result. Closing invalidates UI scopes immediately
while application-owned game and metadata workers drain separately. Suspended
UI retains drafts and defers metadata presentation. Native play joins and drains
its final snapshot before Results, retry or Selection; fallible validation and
thread spawn precede route commit so failure preserves the previous screen.
Buttons and timed note rendering inherit their screen's lifetime. No new crate
or UI toolkit dependency was introduced. Reactive menu/toolkit migration remains
separate work.

Known ceiling: metadata cancellation is cooperative and cannot interrupt an
in-progress filesystem/native metadata call; pending operations fence ordinary
navigation, while application close cancels scopes and drains workers. The
current explicit route graph is bounded to four retained entries; expanding
navigation requires updating that graph and its depth admission.

Rust 1.98.1 app all-target source compilation succeeded on Linux, Windows GNU
and macOS. Headless all-target and WASM graphics-library source checks succeeded.
Scoped formatting and diff whitespace checks succeeded. Model/back-stack/scope
and desktop input/cancellation/session-boundary fixtures were authored and
compiled only. Existing macOS block future-incompatibility and WASM cadence
dead-code warnings remain. Tests, product/GUI/native execution, independent
formal review/security/QA and task acceptance remain explicitly user-deferred.
