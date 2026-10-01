# Native Linux solo pause and input reconciliation

Linux solo live play without network competition now connects F9/Pause/Resume
to the native owner and mixer. The owner recovers coalesced render boundaries,
waits for native output progress and interpolates a host boundary without UI
wall-time substitution. It freezes/resumes Transport and judging, maps keysounds
to playback frames, adjusts presentation discipline by the total silent gap,
and reconciles held-key releases through actual Runtime/capture operations.
Paused new presses stay suppressed until release. UI transitions show native
acknowledgement and cancellation still drains a paused owner. Fresh sessions
isolate desired state; replay Watch and unsupported owners expose no controls.

Known ceiling: boundary interpolation quality remains Unknown; this does not
prove acoustic synchronization. Waiting for presentation can accumulate native
input, and SYN_DROPPED/resync or invalid clock relations explicitly stop and
drain a valid prefix. Windows/macOS/local cohorts/network pause policy, loops,
browser/full widget host and complete native/GUI/replay acceptance remain open.
Tests, apps, GUI, native/device/network execution and formal review/QA remain
user-deferred. Authored fixtures are compiled only, not executed.

Source validation: Linux workspace all-targets, Windows GNU/macOS app all-targets,
headless all-targets and WASM graphics library checks succeeded. Final checks
also include acknowledgement retry after UI-slot contention, validated render
cache fallback and preservation of ordinary unpaired release/repeat handling.
Four common-model fixture groups, a player acknowledgement/cancellation/slot
contention fixture and a desktop capability/admission fixture were authored and
compiled only. Scoped Rust 2024 formatting and diff whitespace checks succeeded;
existing macOS block future compatibility and WASM cadence warnings remain.
