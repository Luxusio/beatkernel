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

The policy-aware capture header now validates actual staged rules and records
the outer timing policy with explicit staged rule schemas. Common section
playback reconstructs those rules, checks original source declarations, head
envelope and hit classes, and compares the complete canonical header. Legacy
single-profile tuple decoders refuse staged recordings instead of discarding
the additional policy.

Native header/capture preparation and admission compare full timing identity,
including when recording is disabled. Shared timing inspection checks actual
stage windows without parsing snapshot bytes. Source difficulty checks preserve
RANK/DEFEX identity even when both resolve to equal numerical windows.

Development integration executes actual Runtime input reports through capture,
wire encoding/decoding, replay, visual replay and repeated seek for both input
modes and finite/unlimited sections. Four native admission/preparation fixtures
cover reverse mismatches, canonical roundtrips and changed/malformed source
declarations. The full no-default-features app library run passed 1,751 tests,
zero failed, four ignored. Independent incremental source review found no new
defect in this integration; final task review/QA still requires all ACs.

StepGameplay/local-member preparation, actual native/browser selectors and
remaining record/competition consumers still need policy-aware wiring. Do not
count WBS08.07 complete from this capture/replay increment.

Policy-aware StepGameplay and local-member constructors now retain selected
windows, gauge, classes, completion and capture/header identity. Native cohort
preparation shares the same staged member helper. Browser preparation resolves
the original source before section filtering and consumes that immutable policy
in both solo and local audio-authoritative constructors. Numerical selection
does not change the explicitly chosen BeatKernel or BMS gauge.

The static browser Timing controls expose the pinned numerical preset,
rank/DEFEX precedence and gauge. Live setup snapshots the selection before
awaiting audio; replay consumes its recorded policy. Settings optionally retain
the three timing fields together; profiles with no selected preset retain their
old version-1 shape. Unsupported combinations return specific errors.

The browser WASM type check passed. Host regression passed 1,764 tests with
four ignored and one new fixture assertion failure; the assertion incorrectly
forbade an unrelated pending-note timeout during wrong-contact cancellation.
After correction, all seven Step policy fixtures passed, including the formerly
failing case. The web regression passed 797 tests with one new finite-end mock
failure. Its author corrected the mock and passed that focused test across six
live construction branches plus legacy/replay; unchanged green tests were not
repeated. Initial omitted WAV declarations and an incompatible Node virtual
address limit were development setup mistakes, corrected without weakening
production validation.

Native CLI selection, generated current browser artifacts, actual browser
acceptance and final independent task review/QA remain open. These results do
not establish whole-player completion or physical audio verification.

## Native selection and record previews

Linux, Windows and macOS launchers accept the paired options
`--timing-preset beatoraja-sevenkeys/8320241d8481e0826c703878c3eba01cd81ca3e4/v1`
and `--rank-precedence rank-first|defexrank-first`. Both options must be supplied
together; duplicate, incomplete and unknown selections are rejected. Gauge
selection remains independent through `--gauge`. With neither timing option,
the existing early/late configuration remains effective.

Native preparation resolves the original chart's difficulty before filtering a
practice section. Solo and local members consume the resulting immutable
policy for judging, completion, recording and competition admission. Record
previews derive the same policy from the current settings and compare full
timing identity. Equal numerical windows with different declaration precedence
do not make records interchangeable.

The focused host library run in
`target/wf/bms-timing-native-preview-focused.log` passed 63 tests with no failures.
It includes the actual record-preview regression, settings-pair admission,
staged live/capture/replay/seek and local-member fixtures. Earlier native chart
preparation fixtures passed eight tests. These are development checks; native
device execution and independent final CLI/browser QA remain separate.

## Independent verification — 2026-10-10

Full-sweep independent code review passed. Independent CLI QA passed 96
focused tests: adapter 7, core staged interactions 11, app timing 63, native
chart preparation 9 and native parser selections 6. Current primary and three
standalone native binaries built; current help and invalid selection commands
were executed. A real selected BMS/WAV fixture reached native preparation before
the deliberately unavailable ALSA endpoint refused. This is not physical audio
or foreign-OS device evidence.

Independent interactive Chromium QA used regenerated WASM SHA-256
`04a5548e5b230dd5a019026680baf1806b921d50d5d0b6263858d053e93ff81e`.
Actual live capture retained all sixteen independently checked stage windows
and classifications. Replay retained hits/misses/combo 4/2/1 despite conflicting
live selectors. Settings roundtrip, legacy settings reset, missing-rank
rejection/recovery and two-member keyboard/touch capture were exercised.
Both member recordings retained the selected numerical policy.

The earlier browser lens returned BLOCKED_ENV: remaining historical display/retry
verification encountered software-GPU device destruction, a render
acknowledgement timeout and a mixer-command rejection. Their environment versus
production causes remain unproven. Evidence and screenshots are under
`target/wf/qa-browser-bms-timing-final-20261010`; CLI evidence is under
the structural final and executed-command transcript of
`qa_cli_bms_timing_20261010_final`. Its commands are reproducible:

```sh
cargo test -p beatkernel-bms --test timing_preset --locked
cargo test -p beatkernel --test profiled_interaction --locked
cargo test -p beatkernel-bms-runtime --no-default-features --lib timing --locked
cargo test -p beatkernel-bms-runtime --no-default-features --lib native_chart --locked
cargo test -p beatkernel-bms-runtime --no-default-features --bin beatkernel-bms-runtime --bin linux_bms --bin windows_bms --bin macos_bms timing_selection --locked
```

The generated BMS/WAV fixtures remain under
`target/wf/bms-timing-independent-cli-fixture` and current executables under
`target/wf/worklet-chronology-qa-cli-1/cargo/debug`. Independent CLI output was
not redirected to log files; executable presence alone is not PASS evidence.
That earlier run did not complete WBS08.07; passing subsets did not replace the
blocked lens. The subsequent current-artifact acceptance below resolves the
remaining browser behavior, while Harness attestation is checked separately.

## Known ceiling

The earlier timing acceptance run could not display stored history while
completed Results owned presentation. Subsequent continuity implementation
added correlated historical acceptance and `BrowserReplay.max_combo`; current
generated bindings expose the latter as unsigned-u64 BigInt. On 2026-10-10,
independent headed QA visibly confirmed current stored history and score after
six-second capture without reprepare. A later run initially hit the actual
gameplay initialization guard; that failed attempt remains preserved.
See [current continuity evidence](../runtime/REQ__browser-historical-record.md#current-headed-acceptance-evidence--2026-10-10).
The subsequent tmpfs-cache connected script also completed current six-second
replay with exact `1/4/0/1` score parity and original timing metadata, then local
two-player keyboard/touch stop. Independent visual inspection is confirmed;
the continuity lens's final verdict is PASS. These follow-up observations do not automatically close the earlier
timing task or complete its remaining WBS08.07 conditions.

## Current selected-policy acceptance — 2026-10-10

Fresh independent interactive browser QA at HEAD `6ac6769` uses main WASM
`197cadc2fe3339d537d0e8460186b391c6137dec08eba5cece719a477d5ccefe`
and unchanged audio WASM `7235be08ccf465e4b67678afa34273abe1d30ed9cdc05aafc8556fa4f42f06dc`.
Original production Worker bytes and deadlines are preserved. Owned headed
Xvfb/Chromium software Vulkan and a private 16 MiB tmpfs cache use the existing
verified [browser setup](../verification/GUIDE__headless-webgpu.md).

Actual selected-policy six-second capture and replay with conflicting live
selectors finish with identical hits/misses/combo/maxCombo `1/4/0/1`. Record
headers retain all sixteen grade/class/early/late rows across four profiles and
the exact numerical/Hold versions. Stored score/provenance, next/back pages and
loaded-record preview seek to 1.25 seconds and back to zero visibly work.
Settings save/load, legacy reset, missing-rank refusal/recovery and selected
stop/retry retain their specified policy. Two local keyboard/touch members both
capture the same selected stage-window metadata from a 0.75-second section.

The independent lens returns PASS; thirteen screenshots are visually inspected,
with zero console/page errors or unexpected Worker failures. Both Node commands
and owned Chromium/Xvfb exit zero; servers close and private profiles/caches are
removed. Preserved earlier failures are not overwritten. Reproduce with
`node target/wf/bms-timing-resume-20261010/run.mjs` and its focused `seek.mjs`;
reports, original traces and screenshots are machine-local in that directory.

Together with preceding full code review and independent CLI96 coverage, this
completes the software numerical-preset integration scope of WBS08.07. Browser
observation does not re-prove every core boundary or competition branch; those
remain the prior fixture/review evidence. Actual device behavior, physical
latency, full source-engine compatibility and the broader player remain separate.
Task closure still requires ordered hook-owned Harness evidence; an actual PASS
final without that evidence is non-attesting.
