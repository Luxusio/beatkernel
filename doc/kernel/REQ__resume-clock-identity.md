# Preserve output identity during gameplay resume

The discipline reconstruction below applies to explicit legacy HOST/correction
consumers; current BMS launchers select the audio-authoritative contract at the
end of this document.

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

## Audio-authoritative native migration

The earlier discipline reconstruction describes the legacy HOST/correction
consumer. Audio-authoritative playback on an unchanged output epoch/basis retains
continuous physical-output/HOST correlation during ordinary pause/resume. Reset
only for an explicit correlation reset while fully paused/drained or a newly
published output epoch; preserve committed input and logical operation history.

An additive audio pause boundary carries the existing conservative original HOST
window and cutoff, plus the actual raw output point from the committed physical
transition frame and its epoch. Point and ASIO interval paths preserve this same
physical evidence; playback/song frames cannot substitute for it. No transition
is emitted while awaiting evidence. Malformed/refused updates retain the owner
and marker. Clear the old marker only on successful output rebind.

Logical Transport control, shared native audio pump selection and atomic
held-output publication now consume this boundary. Production solo/local
launchers select those paths. Portable lifecycle/publication fixtures pass;
physical execution and ordered independent QA remain required for full resume
acceptance. Neither boundary retention nor type checks prove acoustic accuracy.
