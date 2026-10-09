# Versioned BMS timing presets

Timing selection is opt-in. Existing caller windows and recordings keep their
semantics when no preset is selected. Missing difficulty does not synthesize a
default. The BMS adapter resolves numerical windows without native IO; generic
core interactions and the application own their respective execution and
recording policies.

The first numerical preset pins SEVENKEYS hit-window data to beatoraja commit
`8320241d8481e0826c703878c3eba01cd81ca3e4`:
[JudgeProperty](https://github.com/exch-bms2/beatoraja/blob/8320241d8481e0826c703878c3eba01cd81ca3e4/src/bms/player/beatoraja/play/JudgeProperty.java)
and [BMSPlayerRule](https://github.com/exch-bms2/beatoraja/blob/8320241d8481e0826c703878c3eba01cd81ca3e4/src/bms/player/beatoraja/play/BMSPlayerRule.java).
It identifies a numerical table, not complete beatoraja interaction semantics.
BeatKernel Hold release/timeout semantics must have their own recorded version;
plain LN automatic completion and CN/HCN reverse scratch behavior are separate
compatibility work.

Four immutable profiles distinguish key head, scratch head, key LN-end and
scratch LN-end. Each contains PGREAT, GREAT, GOOD and BAD, using explicitly
assigned grade IDs 1 through 4. Empty-hit/miss windows are excluded. Convert
source target-minus-input limits to core input-minus-target early/late extents.

RANK codes 0..4 yield percentages 25/50/75/100/125. Positive integer DEFEXRANK
first yields floor(value × 75 / 100). Scale each original microsecond extent by
that percentage with integer truncation, then convert to nanoseconds. Custom
window rates are fixed at 100. All arithmetic and narrowing are checked.
Fractional and zero DEFEXRANK are explicitly unsupported by this named preset;
their typed metadata remains valid. Zero rejection differs from the source
engine's fallback and must not be described as identical admission.

The core must retain indexed builtin start routing, use selected head/tail
windows for actual admission, grading and deadlines, and apply input offset
once. Application completion must cover the widest actual late extent. Full
effective profiles, numerical version, difficulty source and interaction
semantics must survive capture, replay, checkpoint/seek and competition
identity; equal global envelopes do not prove equal policies.

WBS08.07 remains incomplete until these windows are wired through
ClassifiedWindow, actual native/browser preparation and recorded playback.
Adapter-only tests or numerical resolution do not prove that integration.

## Implementation progress

The adapter resolves the four numerical profiles and registers actual parsed
key/scratch instant/hold rules. Generic core profiled evaluators retain indexed
start eligibility, owner release/cancellation and separate stage deadlines.
Legacy evaluator snapshot bytes retain their original layout; opt-in snapshots
include stage windows and Hold semantics version.

Development verification passed seven adapter tests and eleven core tests,
including literal table oracles, exact boundaries, actual differing lane/tail
grades, offset applied once, tail bounds beyond the routing envelope, ownership,
and same-envelope snapshot mismatch/restore. These tests are not full task QA.
Application ClassifiedWindow/completion, immutable recorded policy, replay and
native/browser selectors remain required in this active task.

The app resolver now retains numerical selection, header precedence and all
four applied profiles beside the existing ClassifiedWindow/gauge policy.
Policy-aware native pristine preparation uses staged rules; cold admission
compares actual builtin stage configuration even without recording. Both
selected-policy/legacy-judge and legacy-policy/staged-judge mismatches refuse.
The core inspection port has no BMS classification or native IO.

An optional outer timing setup codec preserves numerical/Hold versions,
difficulty declaration, effective percentage and every classified extent.
Decoding recomputes the selected table and compares all stored values; absent
timing retains legacy bytes. Seven codec fixtures include real replay-file wire
roundtrips, malformed/version/window rejection and exact header limits. The
app timing-focused development run passed 42 tests after the reverse-identity
review finding was fixed. The subsequent no-default-features app library
regression passed 1,745 tests with zero failures and four ignored; this excludes
desktop/browser feature execution. These codec helpers are not yet connected to every
capture/replay consumer; that integration and actual selectors remain open.

Before the subsequent cold inspection addition, the core/BMS regression run
passed 541 tests including doctests, zero failures/ignored. Its first cold build
hit the bounded 240-second timeout; the follow-up warm run exited successfully.
This is development evidence, not independent final task QA or all-platform
release acceptance.
