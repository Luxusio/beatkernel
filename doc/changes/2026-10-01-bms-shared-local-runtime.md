# Shared local Runtime execution

The local player roster now has an execution composition over the existing core
Runtime. Each member keeps independent bindings, judge, chronology, sequence and
telemetry state. One authoritative Transport and one actual mixer producer are
temporarily installed in the routed member using RAII ownership exchange. This
adds no intermediate audio queues, producer clones or per-step transport copies.
Tiny disconnected placeholders are allocated only at member setup.

Exact devices route input to one player. Unassigned sources return an explicit
ignored result without advancing gameplay. Deadline advancement passes the same
host/output clock points to every member. Setup rejects duplicate players/devices,
incorrect binding selectors, player/BGM voice collisions and excessive telemetry
retention. Checked off-thread voice remapping preserves same-player replacement
aliases and rejects exhaustion before changing accepted mappings.

Partial operation errors retain completed member reports and fence further
judging. RAII restores shared owners even during unwinding; queue admission and
transport access remain available for host cleanup without removing the fence.
The solo adapter preserves committed partial reports for existing replay/UI
publication and retains the existing native API shape.

Linux, Windows and macOS native BMS compositions now construct this solo adapter,
so the group path is adopted by actual production source. Existing native input
clock provenance, calibration, BGM scheduler, completion and cleanup remain the
sources of session timing and lifecycle. Roster UI and multiple simultaneous
native acquisition still need implementation before local multiplayer is playable.

Fixtures are authored for four actual core judges, routing, shared-time deadlines,
voice/setup admission, partial failures and unwind restoration. Execution and
formal review/QA remain user-deferred; compilation alone is not native acceptance.
The original player Goal remains active.

Source checks succeeded on 2026-10-01 for the workspace all-targets on Linux,
Windows GNU/macOS x86_64 app all-targets, headless app all-targets and WASM
`graphics` library. Scoped formatting passed. Existing WASM cadence dead-code
and macOS `block 0.1.6` future-incompatibility warnings remain. Fixtures did not run.
