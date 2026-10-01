# Explicit reusable panel lifecycle

Stateful menu panels expose Active, Retained, Suspended and Disposed phases
derived from the single Navigator. Opening a child retains the parent's draft
and subscriptions; Back reactivates that same instance. Suspend preserves both
parent and child while deferring metadata presentation. Scope exit permanently
cancels its task permits, and metadata admission requires the active instance
and an Active permit. Route cleanup still disposes children before parents;
native session cancellation and joins remain separate. Small controls and notes
inherit their containing UI lifetime, without individual back stacks or hooks.

Known ceiling: this is a bounded typed panel ownership primitive, not a generic
fragment/plugin host. Already-running filesystem calls cannot be interrupted.
Panel suspension does not implement audio transport pause. Coordinated live
pause/loops, browser/full widget host and complete native/GUI acceptance remain
unfinished. Tests, apps, GUI, native/device operations and formal review/QA
remain user-deferred; pure fixtures are authored and compiled only.

Source validation: Linux, Windows GNU and macOS app all-targets checks,
headless all-targets and WASM graphics library checks succeeded. Initial
desktop fixture imports used the binary crate root; corrected them to the
runtime library path and reran all three affected desktop checks successfully.
Scoped Rust 2024 formatting and diff whitespace checks succeeded. Existing
macOS block future compatibility and WASM cadence warnings remain.
