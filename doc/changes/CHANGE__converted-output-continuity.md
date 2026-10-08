# Portable converted output continuity

Status: independent code/security/documentation review and required CLI QA passed.

The additive continuity-enabled converter preserves exact rational source
position, retained past samples and unread lookahead through cold target-rate,
matrix and capacity changes. Every fallible preparation precedes commit;
unrepresentable exact arithmetic refuses without changing subsequent PCM.
Existing fixed-rate constructor paths remain compatible. Zero output preserves
the existing source(empty) reporting contract without advancing converter state.

ConvertedMixer owns the original Mixer and converter together. Construction
failure returns the original Mixer through the existing MixerOpenFailure type.
Read-only access and whole-owner moves preserve unique ownership; successful
continuity transfer cannot silently extract only the Mixer. Reports separate
actual source callbacks, target extent/cursor and exact consumed/pulled positions.
The target cursor counts emitted active and held frames, not native presentation
or elapsed time across rates.

Explicit held rendering writes zeros without invoking the Mixer or advancing
source phase/history/commands, retaining pending PCM for later release. A
source paused report does not prove cached target PCM is silent. Construction
at an advanced Mixer starts a new converter boundary and cannot reconstruct
earlier native history. Native stream migration must retain the full owner from
its beginning; no native pause/end/presentation acknowledgment is invented here.

Development checks passed all 21 converter tests (eight new) and 11 owner tests.
Independent piecewise rational/PCM and sinc-history oracles cover repeated
retargets, refusal and partition boundaries; actual Mixer/queue fixtures cover
ownership, pause/lookahead and finite-end facts. Callback instrumentation reports
zero allocation/reallocation/deallocation on active and held paths. Test setup
PCM budgets and common-oracle denominator were corrected without relaxing
PCM/position/allocation assertions.

Review remediation adds actual thread-local one-shot allocation refusal:
converter window/kernel preparation, every constructor allocation path and
owner retarget preparation. The tests confirm AllocationFailed, unchanged
state/PCM, preserved original Mixer voices/queue, and equivalent successful
retry; injection is disabled before assertions and isolated from other threads.
Updated focused checks pass 22 converter and 14 owner tests. Production code
is unchanged by this test-only correction.

Evidence: `target/wf/converted-output-continuity/converter-development-second.log`
and `owner-development-third.log`; allocation-refusal remediation evidence is
`target/wf/converted-output-continuity/allocation-refusal-final-development.log`.
Final QA at `c06135f` passed 3,037 workspace unit/integration tests and 20
doc-tests (zero failures; six ignored). The focused 49 passing tests are a
subset, not an additional total. A standalone public API PCM/position oracle
also passed. Production browser and browser-audio WASM library checks passed;
Windows/Darwin all-target Rust checks passed with C stubs, without linking or
native execution. Evidence is in
`target/wf/converted-output-continuity/qa-cli-1/evidence-summary.json`.

An additional browser WASM all-target check failed on 16 preexisting app
fixture cfg/import errors. The affected app definitions are unchanged from
`aa037e0`; this remaining Goal defect does not imply production WASM library
failure, and WASM test targets are not claimed to pass. Allocation-refusal
coverage is captured in committed integration tests. The standalone driver
linker setup is task-local evidence; its recipe is not promoted to a runbook.

Unequal native rate enablement, full-owner
retirement/recovery, target-grid evidence, pause/end/publication migration and
supported-host/physical measurements remain mandatory later Goal work. BK019
and the full player are not complete merely because this component exists.
