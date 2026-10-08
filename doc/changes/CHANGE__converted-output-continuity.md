# Portable converted output continuity

Status: development checks passed; independent review and final QA pending.

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

Evidence: `target/wf/converted-output-continuity/converter-development-second.log`
and `owner-development-third.log`. Unequal native rate enablement, full-owner
retirement/recovery, target-grid evidence, pause/end/publication migration and
supported-host/physical measurements remain mandatory later Goal work. BK019
and the full player are not complete merely because this component exists.
