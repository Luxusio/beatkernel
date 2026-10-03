# Opt-in button and contact press interactions

Add explicit press instant/hold evaluators on the common judge path. They accept
actual button and touch events without replacing a touch contact with a key.
Hold ownership retains device, surface/control, logical destination and contact.
Repeated Down and Move are not new starts. Only an owning Up can grade a tail;
an owning Cancel ends the hold as RejectedInput rather than a successful tail.

Use the existing profile-window start index for fresh button/contact presses.
Contact held state is deterministic, snapshot-clonable and restorable. New
interaction/contact-state identities distinguish the opt-in configuration.
Existing button-only evaluator behavior and canonical state bytes remain the
compatibility contract; BMS default rules do not change in this slice.

BMS profile/replay setup integration and browser contact acquisition remain
necessary before playable touch support is complete. Source and six independent
actual JudgeEngine/ReplaySession fixture groups are authored. No runtime, test/assertion, benchmark or QA
execution is authorized. Both writers reported STOPPED before scoped formatting. Workspace/headless and
both WASM feature cargo checks exited 0; WASM retains three existing cadence
dead-code warnings. Compilation does not prove fixture or hardware acceptance.
The full Goal remains active; required independent reviews and QA precede close.
