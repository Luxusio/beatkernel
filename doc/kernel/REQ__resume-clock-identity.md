# Preserve output identity during gameplay resume

An ordinary pause/resume on an unchanged output stream must not reset its
observation epoch to zero or replace user-selected clock discipline settings
with defaults. Actual common solo and local-cohort loops stage a fresh
presentation owner through the business-owned GameplayPresentationPort.
Core and native adapters expose their actual configuration. Existing custom
ports retain the prior default configuration unless they override that getter.

The staged owner retains the existing epoch, including u64::MAX without an
increment, and the explicitly supplied stream/playback/host/song origins.
Fresh observation history still requires original device evidence and warmup.
Construction, identity restoration and native seeding complete before replacing
the old presentation owner. Refusal leaves that owner intact. Epoch-capable
custom ports must not silently fall back to untagged reconstruction; reject a
new owner that cannot represent the old epoch.

This is same-stream resume, distinct from a backend or buffer discontinuity:
those require a strictly newer epoch, physical buffer handling and new output
anchoring. No operating-system business branches, new crates, per-note work,
callback allocation or live backend switch are introduced. The cold staged
constructor allocates observation storage as before. Existing transport/pause
chronology and seed evidence paths remain unchanged. Clock publication follows
[resume commit order](REQ__resume-commit-order.md): stage both clocks and require
accepted seed evidence before replacing either. Device and NativePause effects
are not rolled back by this software publication order.

Author independent port cases for nonzero/max epochs, configuration/origin
preservation, unsupported identity and refusal atomicity, plus actual common
solo/cohort memory-loop resume cases. Assertions and runtime/formal review/QA
remain deferred. Run scoped formatting and the four existing sequential
compile-only configurations only after both paired writers stop. Full BMS
player Goal and automatic live handoff remain incomplete.
